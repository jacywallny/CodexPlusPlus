use std::{
    cell::{Cell, RefCell},
    time::Duration,
};

use serde_json::json;
use tokio::time::Instant;

use crate::settings::{BackendSettings, RelayProfile};

const DEFAULT_ROUTE_SECS: u64 = 240;

pub(super) fn route_deadline(settings: &BackendSettings) -> Instant {
    let inherited = crate::protocol_proxy::configured_stream_header_timeout(
        settings.upstream_stream_header_timeout_secs,
    );
    let seconds = settings
        .gateway_stream_header_budget_secs
        .unwrap_or(DEFAULT_ROUTE_SECS.max(inherited.as_secs()));
    Instant::now() + explicit_timeout(seconds)
}

fn explicit_timeout(seconds: u64) -> Duration {
    Duration::from_secs(seconds.clamp(1, 3600))
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum HeaderWaitError {
    #[error("响应头等待预算已用尽（{scope}），停止发送下一次上游请求")]
    BudgetExhausted { scope: &'static str, sent: bool },
    #[error(
        "供应商「{provider}」在 {elapsed_ms} ms 内未返回 HTTP 响应头（{scope}），上游执行结果未知，已停止自动重发"
    )]
    Timeout {
        provider: String,
        elapsed_ms: u64,
        scope: &'static str,
    },
    #[error("供应商「{provider}」在返回 HTTP 响应头前连接中断，上游执行结果未知，已停止自动重发")]
    Disconnected { provider: String },
    #[error("供应商「{provider}」返回 HTTP {status} 重定向，请使用供应商最终的 API 地址")]
    Redirect { provider: String, status: u16 },
}

impl HeaderWaitError {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::BudgetExhausted { .. } => "gateway_header_budget_exhausted",
            Self::Timeout { .. } => "gateway_upstream_header_timeout",
            Self::Disconnected { .. } => "gateway_upstream_outcome_unknown",
            Self::Redirect { .. } => "gateway_upstream_redirect",
        }
    }
}

pub(crate) struct HttpAttempt {
    request_id: Option<String>,
    candidate: u32,
    provider: String,
    relay_id: String,
    model: RefCell<String>,
    deadline: Instant,
    budget_scope: &'static str,
    queued: Duration,
    sends: Cell<u32>,
}

tokio::task_local! {
    pub(crate) static HTTP_ATTEMPT: HttpAttempt;
}

impl HttpAttempt {
    pub(super) fn new(
        settings: &BackendSettings,
        route_deadline: Instant,
        relay: &RelayProfile,
        model: &str,
        request_id: Option<&str>,
        candidate: u32,
        queued: Duration,
    ) -> Self {
        let configured = settings
            .gateway_provider_stream_header_timeout_secs
            .get(&relay.id)
            .copied();
        let timeout = configured.map(explicit_timeout).unwrap_or_else(|| {
            crate::protocol_proxy::configured_stream_header_timeout(
                settings.upstream_stream_header_timeout_secs,
            )
        });
        let provider_deadline = Instant::now() + timeout;
        Self {
            request_id: request_id.map(str::to_owned),
            candidate,
            provider: super::accounts::mask_account_name(&relay.name),
            relay_id: relay.id.clone(),
            model: RefCell::new(model.to_owned()),
            deadline: route_deadline.min(provider_deadline),
            budget_scope: if route_deadline <= provider_deadline {
                "请求总预算"
            } else {
                "供应商预算"
            },
            queued,
            sends: Cell::new(0),
        }
    }

    pub(crate) fn deadline(&self) -> Instant {
        self.deadline
    }

    pub(crate) fn bind_model(&self, model: &str) {
        (*self.model.borrow_mut()).replace_range(.., model);
    }

    pub(crate) fn routing_attempt(&self, attempt: u32) -> u32 {
        self.candidate.saturating_sub(1) + attempt
    }

    pub(crate) fn budget_error(&self) -> HeaderWaitError {
        HeaderWaitError::BudgetExhausted {
            scope: self.budget_scope,
            sent: self.sends.get() > 0,
        }
    }

    pub(crate) fn timeout_error(&self, elapsed_ms: u64) -> HeaderWaitError {
        HeaderWaitError::Timeout {
            provider: self.provider.clone(),
            elapsed_ms,
            scope: self.budget_scope,
        }
    }

    pub(crate) fn disconnected_error(&self) -> HeaderWaitError {
        HeaderWaitError::Disconnected {
            provider: self.provider.clone(),
        }
    }

    pub(crate) fn redirect_error(&self, status: u16) -> HeaderWaitError {
        HeaderWaitError::Redirect {
            provider: self.provider.clone(),
            status,
        }
    }

    pub(crate) fn begin_send(&self, request: &reqwest::Request) -> HttpSend {
        let attempt = self.sends.get() + 1;
        self.sends.set(attempt);
        HttpSend {
            attempt,
            body_bytes: request
                .body()
                .and_then(|body| body.as_bytes())
                .map(<[u8]>::len),
            started_at_ms: crate::launcher::current_time_ms(),
            started: Instant::now(),
        }
    }

    pub(crate) fn record(
        &self,
        send: &HttpSend,
        result: &'static str,
        status: Option<u16>,
        retry: bool,
    ) {
        let _ = crate::diagnostic_log::append_diagnostic_log(
            "gateway.http_upstream_attempt",
            json!({
                "requestId": self.request_id,
                "candidate": self.candidate,
                "attempt": send.attempt,
                "relayId": self.relay_id,
                "relayName": self.provider,
                "upstreamModel": self.model.borrow().as_str(),
                "bodyBytes": send.body_bytes,
                "startedAtMs": send.started_at_ms,
                "elapsedMs": send.elapsed_ms(),
                "queueWaitMs": self.queued.as_millis() as u64,
                "headerBudgetMs": self.deadline.saturating_duration_since(send.started).as_millis() as u64,
                "phase": "response_headers",
                "result": result,
                "statusCode": status,
                "retry": retry,
            }),
        );
    }
}

pub(super) fn route_budget_exhausted(
    request_id: Option<&str>,
    phase: &'static str,
    queued: Duration,
) -> HeaderWaitError {
    let _ = crate::diagnostic_log::append_diagnostic_log(
        "gateway.http_header_budget_exhausted",
        json!({
            "requestId": request_id,
            "phase": phase,
            "queueWaitMs": queued.as_millis() as u64,
        }),
    );
    HeaderWaitError::BudgetExhausted {
        scope: "请求总预算",
        sent: false,
    }
}

pub(crate) struct HttpSend {
    attempt: u32,
    body_bytes: Option<usize>,
    started_at_ms: u64,
    started: Instant,
}

impl HttpSend {
    pub(crate) fn elapsed_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }
}
