use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use serde_json::json;

use super::http_wait::HeadersReceived;
use super::route::{GatewayCandidate, RoutePlan};
use crate::settings::BackendSettings;

const FAILURE_LIMIT: u8 = 2;
const COOLDOWN: Duration = Duration::from_secs(30);

static LANES: LazyLock<Mutex<HashMap<LaneKey, Circuit>>> = LazyLock::new(Mutex::default);

#[derive(Clone, Eq, Hash, PartialEq)]
struct LaneKey {
    provider: String,
    model: String,
}

#[derive(Default)]
struct Circuit {
    generation: u64,
    failures: u8,
    reopen_at: Option<Instant>,
    probing: bool,
}

#[derive(Clone, Copy)]
struct Permit {
    generation: u64,
    probe: bool,
}

impl Circuit {
    fn acquire(&mut self, now: Instant) -> Result<Permit, Duration> {
        let probe = if let Some(reopen_at) = self.reopen_at {
            if self.probing {
                return Err(Duration::from_secs(1));
            }
            if reopen_at > now {
                return Err(reopen_at.duration_since(now));
            }
            self.probing = true;
            true
        } else {
            false
        };
        Ok(Permit {
            generation: self.generation,
            probe,
        })
    }

    fn finish(&mut self, permit: Permit, end: End, now: Instant) {
        if permit.generation != self.generation {
            return;
        }
        if permit.probe {
            self.probing = false;
        }
        if end == End::Completed {
            self.failures = 0;
            if self.reopen_at.take().is_some() {
                self.generation += 1;
            }
        } else if end.unhealthy() {
            self.failures = self.failures.saturating_add(1);
            if permit.probe || self.failures >= FAILURE_LIMIT {
                self.generation += 1;
                self.reopen_at = Some(now + COOLDOWN);
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum End {
    Completed,
    HeaderTimeout,
    RouteHeaderTimeout,
    HeaderDisconnect,
    ConnectError,
    HttpUnavailable,
    FirstOutputTimeout,
    BodyIdleTimeout,
    StreamReadError,
    MissingTerminal,
    UpstreamTerminalError,
    UpstreamOverloaded,
    DownstreamCancelled,
}

impl End {
    fn label(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::HeaderTimeout => "header_timeout",
            Self::RouteHeaderTimeout => "route_header_timeout",
            Self::HeaderDisconnect => "header_disconnect",
            Self::ConnectError => "connect_error",
            Self::HttpUnavailable => "http_unavailable",
            Self::FirstOutputTimeout => "first_output_timeout",
            Self::BodyIdleTimeout => "body_idle_timeout",
            Self::StreamReadError => "stream_read_error",
            Self::MissingTerminal => "missing_terminal",
            Self::UpstreamTerminalError => "upstream_terminal_error",
            Self::UpstreamOverloaded => "upstream_overloaded",
            Self::DownstreamCancelled => "downstream_cancelled",
        }
    }

    fn unhealthy(self) -> bool {
        matches!(
            self,
            Self::HeaderTimeout
                | Self::HeaderDisconnect
                | Self::ConnectError
                | Self::HttpUnavailable
                | Self::FirstOutputTimeout
                | Self::BodyIdleTimeout
                | Self::StreamReadError
                | Self::MissingTerminal
                | Self::UpstreamOverloaded
        )
    }
}

struct LanePermit {
    key: LaneKey,
    permit: Option<Permit>,
}

impl LanePermit {
    fn finish(&mut self, end: End) {
        if let Some(permit) = self.permit.take()
            && let Some(circuit) = LANES.lock().unwrap().get_mut(&self.key)
        {
            circuit.finish(permit, end, Instant::now());
        }
    }
}

impl Drop for LanePermit {
    fn drop(&mut self) {
        self.finish(End::DownstreamCancelled);
    }
}

pub(super) struct Attempt {
    key: LaneKey,
    request_id: Option<String>,
    candidate: u32,
    send: Option<u32>,
    started: Instant,
    permit: Option<LanePermit>,
}

impl Attempt {
    pub(super) fn acquire(
        candidate: &GatewayCandidate,
        request_id: Option<&str>,
        attempt: u32,
        schedule: bool,
    ) -> Result<Self, Duration> {
        let key = LaneKey {
            provider: candidate.relay.id.clone(),
            model: candidate.upstream_model.clone(),
        };
        let now = Instant::now();
        let permit = if schedule {
            let permit = LANES
                .lock()
                .unwrap()
                .entry(key.clone())
                .or_default()
                .acquire(now)?;
            Some(LanePermit {
                key: key.clone(),
                permit: Some(permit),
            })
        } else {
            None
        };
        Ok(Self {
            key,
            request_id: request_id.map(str::to_owned),
            candidate: attempt,
            send: None,
            started: now,
            permit,
        })
    }

    pub(super) fn finish_before_body(mut self, end: End, status: Option<u16>) {
        self.record(end, status, None, None, None);
        if let Some(permit) = self.permit.as_mut() {
            permit.finish(end);
        }
    }

    pub(super) fn headers(
        mut self,
        response: &mut reqwest::Response,
        settings: &BackendSettings,
        complete: Completion,
    ) {
        let bounded = |seconds: Option<u64>| {
            seconds
                .filter(|seconds| *seconds > 0)
                .map(|seconds| Duration::from_secs(seconds.min(3600)))
        };
        let received = if let Some(headers) = response.extensions_mut().remove::<HeadersReceived>()
        {
            self.started = headers.sent;
            self.send = Some(headers.attempt);
            headers.received
        } else {
            Instant::now()
        };
        let health = Body {
            attempt: Some(self),
            headers: received,
            status: response.status().as_u16(),
            http_version: format!("{:?}", response.version()),
            first_byte: None,
            first_output: None,
            first_output_timeout: bounded(settings.gateway_stream_first_output_timeout_secs),
            body_idle_timeout: bounded(settings.gateway_stream_body_idle_timeout_secs),
            complete: Some(complete),
        };
        response
            .extensions_mut()
            .insert(Attached(Arc::new(Mutex::new(Some(health)))));
    }

    fn record(
        &self,
        end: End,
        status: Option<u16>,
        http_version: Option<&str>,
        headers: Option<Instant>,
        body_times: Option<(Option<Instant>, Option<Instant>)>,
    ) {
        let elapsed_ms =
            |at: Instant| at.saturating_duration_since(self.started).as_millis() as u64;
        let (first_byte, first_output) = body_times.unwrap_or_default();
        let _ = crate::diagnostic_log::append_diagnostic_log(
            "gateway.http_stream_outcome",
            json!({
                "requestId": self.request_id,
                "candidate": self.candidate,
                "attempt": self.send,
                "relayId": self.key.provider,
                "model": self.key.model,
                "status": status,
                "httpVersion": http_version,
                "headersMs": headers.map(elapsed_ms),
                "firstBodyMs": first_byte.map(elapsed_ms),
                "firstOutputMs": first_output.map(elapsed_ms),
                "elapsedMs": elapsed_ms(Instant::now()),
                "outcome": end.label(),
                "halfOpen": self.permit.as_ref().is_some_and(|permit| permit.permit.is_some_and(|permit| permit.probe)),
            }),
        );
    }
}

pub(super) struct Completion {
    pub plan: Option<RoutePlan>,
    pub candidate: GatewayCandidate,
    pub key_conversation: Option<String>,
}

#[derive(Clone)]
struct Attached(Arc<Mutex<Option<Body>>>);

pub(crate) struct Body {
    attempt: Option<Attempt>,
    headers: Instant,
    status: u16,
    http_version: String,
    first_byte: Option<Instant>,
    first_output: Option<Instant>,
    first_output_timeout: Option<Duration>,
    body_idle_timeout: Option<Duration>,
    complete: Option<Completion>,
}

impl Body {
    pub(crate) fn take(response: &mut reqwest::Response) -> Option<Self> {
        response
            .extensions_mut()
            .remove::<Attached>()?
            .0
            .lock()
            .unwrap()
            .take()
    }

    pub(crate) fn observe(&mut self, nonempty: bool, progress: bool) {
        let now = Instant::now();
        if nonempty {
            self.first_byte.get_or_insert(now);
        }
        if progress {
            self.first_output.get_or_insert(now);
        }
    }

    pub(crate) fn deadline(&self) -> Option<(Instant, End)> {
        let first = self
            .first_output
            .is_none()
            .then_some(self.first_output_timeout)
            .flatten()
            .map(|timeout| (self.headers + timeout, End::FirstOutputTimeout));
        let idle = self
            .body_idle_timeout
            .map(|timeout| (Instant::now() + timeout, End::BodyIdleTimeout));
        [first, idle]
            .into_iter()
            .flatten()
            .min_by_key(|(at, _)| *at)
    }

    pub(crate) fn finish(&mut self, end: End) {
        let Some(mut attempt) = self.attempt.take() else {
            return;
        };
        let health_routing = attempt.permit.is_some();
        attempt.record(
            end,
            Some(self.status),
            Some(&self.http_version),
            Some(self.headers),
            Some((self.first_byte, self.first_output)),
        );
        if let Some(permit) = attempt.permit.as_mut() {
            permit.finish(end);
        }
        if let Some(complete) = self.complete.take() {
            if end == End::Completed {
                super::key_pool::record_success(
                    &complete.candidate.key,
                    complete.key_conversation.as_deref(),
                );
                super::speed::record(
                    &attempt.key.provider,
                    &attempt.key.model,
                    if health_routing {
                        self.first_output.unwrap_or(self.headers)
                    } else {
                        self.headers
                    }
                    .saturating_duration_since(attempt.started),
                );
                if let Some(plan) = &complete.plan {
                    super::route::remember_answer(plan, &complete.candidate);
                }
            } else if health_routing && end.unhealthy() {
                super::speed::record_failure(&attempt.key.provider, &attempt.key.model);
            }
        }
    }
}

impl Drop for Body {
    fn drop(&mut self) {
        self.finish(End::DownstreamCancelled);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_open_ignores_stale_completion_and_allows_only_one_recovery() {
        let mut circuit = Circuit::default();
        let now = Instant::now();
        let stale = circuit.acquire(now).unwrap();
        for end in [End::HeaderTimeout, End::MissingTerminal] {
            let permit = circuit.acquire(now).unwrap();
            circuit.finish(permit, end, now);
        }
        circuit.finish(stale, End::Completed, now);
        assert!(circuit.acquire(now).is_err());
        let reopen = now + COOLDOWN;
        let probe = circuit.acquire(reopen).unwrap();
        assert!(probe.probe);
        assert!(circuit.acquire(reopen).is_err());
        circuit.finish(probe, End::DownstreamCancelled, reopen);
        let probe = circuit.acquire(reopen).unwrap();
        circuit.finish(probe, End::BodyIdleTimeout, reopen);
        assert!(circuit.acquire(reopen).is_err());
        let recovered = reopen + COOLDOWN;
        let probe = circuit.acquire(recovered).unwrap();
        circuit.finish(probe, End::Completed, recovered);
        assert!(!circuit.acquire(recovered).unwrap().probe);
        assert!(!circuit.acquire(recovered).unwrap().probe);
    }

    #[test]
    fn client_cancel_and_application_failure_are_neutral() {
        let mut circuit = Circuit::default();
        let now = Instant::now();
        for end in [
            End::DownstreamCancelled,
            End::UpstreamTerminalError,
            End::RouteHeaderTimeout,
        ] {
            for _attempt in 0..3 {
                let permit = circuit.acquire(now).unwrap();
                circuit.finish(permit, end, now);
            }
        }
        assert_eq!(circuit.failures, 0);
        assert!(!circuit.acquire(now).unwrap().probe);
    }
}
