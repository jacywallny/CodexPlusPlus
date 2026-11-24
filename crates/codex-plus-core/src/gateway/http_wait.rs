use std::{
    cell::{Cell, RefCell},
    time::Duration,
};

use serde_json::json;
use tokio::time::Instant;

use crate::settings::{BackendSettings, RelayProfile};

const DEFAULT_ROUTE_SECS: u64 = 240;
pub(super) const ROUTE_BUDGET_SCOPE: &str = "请求总预算";

pub(super) fn route_deadline(settings: &BackendSettings) -> Instant {
    Instant::now() + route_timeout(settings)
}

fn route_timeout(settings: &BackendSettings) -> Duration {
    let inherited = crate::protocol_proxy::configured_stream_header_timeout(
        settings.upstream_stream_header_timeout_secs,
    );
    let seconds = settings
        .gateway_stream_header_budget_secs
        .unwrap_or(DEFAULT_ROUTE_SECS.max(inherited.as_secs()));
    explicit_timeout(seconds)
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
        eligible_for_backup: bool,
        upstream_model: String,
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
    forward_native_stream: bool,
    continuation_timeout: Duration,
    headers_seen: Cell<bool>,
    backup: bool,
    track_dispatch: bool,
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
        forward_native_stream: bool,
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
                ROUTE_BUDGET_SCOPE
            } else {
                "供应商预算"
            },
            queued,
            sends: Cell::new(0),
            forward_native_stream,
            continuation_timeout: timeout.min(route_timeout(settings)),
            headers_seen: Cell::new(false),
            backup: false,
            track_dispatch: settings.gateway_header_backup.enabled,
        }
    }

    pub(super) fn as_backup(mut self) -> Self {
        self.backup = true;
        self.budget_scope = "备用预算";
        self
    }

    pub(crate) fn is_backup(&self) -> bool {
        self.backup
    }

    pub(crate) fn backup_target(&self) -> Option<(String, String)> {
        self.backup.then(|| (self.relay_id.clone(), self.model.borrow().clone()))
    }

    pub(crate) fn track_dispatch(&self) -> bool {
        self.track_dispatch && !self.backup
    }

    pub(crate) fn sent(&self) -> bool {
        self.sends.get() != 0
    }

    pub(crate) fn mark_headers_received(&self) {
        self.headers_seen.set(true);
    }

    pub(crate) fn deadline(&self) -> Instant {
        self.deadline
    }

    pub(crate) fn forward_native_stream(&self) -> bool {
        self.forward_native_stream
    }

    pub(crate) fn continuation_header_timeout(&self) -> Duration {
        self.continuation_timeout
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

    pub(crate) fn timeout_error(&self, elapsed_ms: u64, body_dispatched: bool) -> HeaderWaitError {
        HeaderWaitError::Timeout {
            provider: self.provider.clone(),
            elapsed_ms,
            scope: self.budget_scope,
            eligible_for_backup: body_dispatched && !self.headers_seen.get() && !self.backup,
            upstream_model: self.model.borrow().clone(),
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
        if self.backup && let Some(id) = &self.request_id {
            crate::request_log::update_running_phase(id, "header_backup");
        }
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
        error: Option<&reqwest::Error>,
    ) {
        let mut detail = json!({
                "requestId": self.request_id,
                "candidate": self.candidate,
                "headerBackup": self.backup,
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
        });
        if let Some(error) = error {
            detail["transportError"] = transport_error_detail(error);
        }
        let _ =
            crate::diagnostic_log::append_diagnostic_log("gateway.http_upstream_attempt", detail);
    }
}

fn transport_error_detail(error: &reqwest::Error) -> serde_json::Value {
    use std::error::Error;

    // 从 source 开始，避免把 reqwest 的请求 URL 写进日志。
    let mut current = error.source();
    let mut causes = Vec::new();
    let mut io_kind = None;
    let mut os_code = None;
    while let Some(cause) = current {
        if let Some(io) = cause.downcast_ref::<std::io::Error>() {
            io_kind = Some(format!("{:?}", io.kind()));
            os_code = io.raw_os_error().or(os_code);
        }
        let message = cause.to_string();
        let lower = message.to_ascii_lowercase();
        let sensitive = [
            "://",
            "authorization",
            "bearer ",
            "token=",
            "api_key=",
            "api-key",
            "password=",
        ]
        .iter()
        .any(|marker| lower.contains(marker));
        causes.push(if sensitive {
            "[redacted]".to_owned()
        } else {
            message
                .chars()
                .filter(|c| !c.is_control())
                .take(400)
                .collect()
        });
        current = cause.source();
        if causes.len() >= 12 {
            break;
        }
    }
    json!({"causes": causes, "ioKind": io_kind, "osCode": os_code,
           "connect": error.is_connect(), "timeout": error.is_timeout()})
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
        scope: ROUTE_BUDGET_SCOPE,
        sent: false,
    }
}

pub(crate) struct HttpSend {
    attempt: u32,
    body_bytes: Option<usize>,
    started_at_ms: u64,
    started: Instant,
}

#[cfg(test)]
mod transport_tests {
    use super::transport_error_detail;
    use tokio::io::AsyncReadExt;

    #[tokio::test]
    async fn transport_error_detail_preserves_pre_header_close_without_request_secrets() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let peer = tokio::spawn(async move {
            let (mut connection, _) = listener.accept().await.unwrap();
            let mut bytes = [0; 4096];
            assert!(connection.read(&mut bytes).await.unwrap() > 0);
        });
        let failure = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .post(format!("http://{address}/?token=private-test-value"))
            .bearer_auth("secret-auth-value")
            .body("private-body")
            .send()
            .await
            .unwrap_err();
        peer.await.unwrap();
        let detail = transport_error_detail(&failure);
        assert_eq!(detail["connect"], false);
        assert!(detail["causes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|cause| cause
                .as_str()
                .is_some_and(|s| !s.is_empty() && s != "[redacted]")));
        let serialized = detail.to_string();
        for secret in ["private-test-value", "secret-auth-value", "private-body"] {
            assert!(!serialized.contains(secret));
        }
        println!("loopback diagnostic: {serialized}");
    }
}

#[derive(Clone, Copy)]
pub(crate) struct HeadersReceived {
    pub sent: std::time::Instant,
    pub received: std::time::Instant,
    pub attempt: u32,
}

impl HttpSend {
    pub(crate) fn elapsed_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    pub(crate) fn headers_received(&self) -> HeadersReceived {
        HeadersReceived {
            sent: self.started.into_std(),
            received: Instant::now().into_std(),
            attempt: self.attempt,
        }
    }
}
