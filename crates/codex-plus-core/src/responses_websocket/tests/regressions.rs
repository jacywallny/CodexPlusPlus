use super::*;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

#[tokio::test]
async fn gateway_http_official_pre_output_failover() {
    if isolated("regressions::gateway_http_official_pre_output_failover").await {
        return;
    }
    // The long fixture exceeds 270k estimated tokens; neither request reaches a real provider.
    for (case, model, length, first_event, expected_backup) in [
        (0, "gpt-6.2-sol", 24, None, false),
        (1, "gpt-6.2-astra", 1_100_000, Some(r#"{"type":"response.created"}"#), true),
        (2, "gpt-6.2-sol", 1_100_000, Some(r#"{"type":"response.output_text.delta","delta":"official-output"}"#), false),
        (3, "gpt-6.2-astra", 24, Some(r#"{"type":"response.output_item.added","item":{"type":"function_call","id":"fc_1","name":"test","call_id":"call_1","arguments":""}}"#), false),
        (4, "gpt-6.2-sol", 24, Some(r#"{"type":"response.reasoning_summary_text.delta","delta":"official-reasoning","item_id":"rs_1","summary_index":0,"output_index":0}"#), false),
    ] {
        let primary = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let backup = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let primary_addr = primary.local_addr().unwrap();
        let backup_addr = backup.local_addr().unwrap();
        unsafe {
            std::env::set_var("CODEX_PLUS_OFFICIAL_BASE_URL", format!("http://{primary_addr}/v1"));
        }
        let mut official = profile(primary_addr);
        official["id"] = json!(format!("official-{case}"));
        official["relayMode"] = json!("official");
        official["upstreamBaseUrl"] = json!("");
        official["apiKey"] = json!("");
        official["apiKeys"] = json!([]);
        official["activeApiKeyId"] = json!("");
        official["modelList"] = json!(model);
        let auth = json!({
            "auth_mode":"chatgpt","codex_plus_gateway":true,
            "tokens":{"account_id":"test-account","access_token":"test-access",
                "refresh_token":"test-refresh"}
        });
        official["authContents"] = Value::String(auth.to_string());
        let mut relay = profile(backup_addr);
        relay["id"] = json!(format!("molifang-{case}"));
        relay["name"] = json!("molifang");
        relay["apiKeys"][0]["id"] = json!(format!("backup-{case}"));
        relay["activeApiKeyId"] = json!(format!("backup-{case}"));
        relay["modelList"] = json!(model);
        let mut second_relay = relay.clone();
        second_relay["id"] = json!(format!("nanlab-{case}"));
        second_relay["name"] = json!("NaNLab");
        let fixture = Fixture::new(official.clone());
        fixture.gateway_profiles(vec![relay, second_relay, official]);
        let store = GatewayStore::default();
        let mut settings = store.load().unwrap();
        settings.auto_models = true;
        store.save(&settings).unwrap();
        let primary_task = tokio::spawn(async move {
            let (mut socket, _) = primary.accept().await.unwrap();
            let body = http_fixture_body(&mut socket).await;
            assert_eq!(body["model"], model);
            assert_eq!(body["service_tier"], "priority");
            if let Some(event) = first_event {
                let bytes = format!("data: {event}\n\n");
                let headers = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    bytes.len() + 100,
                );
                socket.write_all(headers.as_bytes()).await.unwrap();
                socket.write_all(bytes.as_bytes()).await.unwrap();
            }
            socket.shutdown().await.unwrap();
        });
        let backup_task = tokio::spawn(async move {
            if !expected_backup {
                assert!(tokio::time::timeout(Duration::from_millis(500), backup.accept()).await.is_err());
                return;
            }
            let (mut socket, _) = backup.accept().await.unwrap();
            let body = http_fixture_body(&mut socket).await;
            assert_eq!(body["model"], model);
            assert!(body.get("service_tier").is_none());
            assert_eq!(body["input"], "a".repeat(length));
            assert_eq!(body["prompt_cache_key"], format!("fast-{case}"));
            assert_eq!(body["reasoning"]["effort"], "high");
            let content = format!(
                "data: {{\"type\":\"response.output_text.delta\",\"delta\":\"backup-output\"}}\n\n\
                 data: {}\n\n",
                completed("backup", ""),
            );
            let headers = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                content.len(),
            );
            socket.write_all(headers.as_bytes()).await.unwrap();
            socket.write_all(content.as_bytes()).await.unwrap();
        });
        let (url, task) = helper(1).await;
        let mut response = reqwest::Client::builder().no_proxy().build().unwrap()
            .post(format!("{}/gw/codex/v1/responses", url.replacen("ws://", "http://", 1)))
            .bearer_auth("local-test")
            .json(&json!({"model":model,"input":"a".repeat(length),"stream":true,
                "service_tier":"priority","prompt_cache_key":format!("fast-{case}"),
                "reasoning":{"effort":"high"}}))
            .send().await.unwrap();
        if first_event.is_none() {
            assert_eq!(response.status(), 400, "case {case}");
            let body: Value = response.json().await.unwrap();
            assert_eq!(body["error"]["code"], "gateway_upstream_outcome_unknown");
        } else if expected_backup {
            assert_eq!(response.status(), 200, "case {case}");
            let body = response.text().await.unwrap();
            assert!(body.contains("backup-output"), "case {case}: {body}");
            assert!(!body.contains("response.created"));
        } else {
            assert_eq!(response.status(), 200, "case {case}");
            let mut body = Vec::new();
            while let Ok(Some(chunk)) = response.chunk().await {
                body.extend_from_slice(&chunk);
            }
            let body = String::from_utf8(body).unwrap();
            let marker = match case {
                2 => "official-output",
                3 => "function_call",
                _ => "official-reasoning",
            };
            assert!(body.contains(marker), "case {case}: {body}");
            assert!(!body.contains("backup-output"));
        }
        finish(task).await;
        finish(primary_task).await;
        finish(backup_task).await;
    }
}

#[derive(Clone, Copy)]
struct HttpFixtureResponse {
    status: &'static str,
    header_delay_ms: u64,
    output_delay_ms: u64,
    truncate_stream: bool,
    close_before_headers: bool,
}

async fn scheduled_http_upstream(
    listener: tokio::net::TcpListener,
    served: Option<HttpFixtureResponse>,
    model: &str,
    headers_timeout: bool,
    redirect: Option<std::net::SocketAddr>,
) {
    if let Some(served) = served {
        let (mut socket, _) = listener.accept().await.unwrap();
        assert_eq!(http_fixture_body(&mut socket).await["model"], model);
        if served.close_before_headers {
            drop(socket);
            assert!(tokio::time::timeout(Duration::from_millis(300), listener.accept()).await.is_err());
            return;
        }
        tokio::time::sleep(Duration::from_millis(served.header_delay_ms)).await;
        let body = if served.status == "200 OK" {
            let delta =
                "data: {\"type\":\"response.output_text.delta\",\"delta\":\"fixture-output\"}\n\n";
            if served.truncate_stream {
                delta.to_string()
            } else {
                format!("{delta}data: {}\n\n", completed("fixture", ""))
            }
        } else {
            "{}".to_string()
        };
        let length = body.len() + if served.truncate_stream { 100 } else { 0 };
        let content_type = if served.status == "200 OK" {
            "text/event-stream"
        } else {
            "application/json"
        };
        let location = redirect.map_or(String::new(), |address| {
            format!("Location: http://{address}/v1/responses\r\n")
        });
        let headers = format!(
            "HTTP/1.1 {}\r\nContent-Type: {content_type}\r\nContent-Length: {length}\r\nConnection: close\r\n{location}\r\n",
            served.status
        );
        let header_write = socket.write_all(headers.as_bytes()).await;
        if !headers_timeout {
            header_write.unwrap();
            tokio::time::sleep(Duration::from_millis(served.output_delay_ms)).await;
            if redirect.is_none() {
                socket.write_all(body.as_bytes()).await.unwrap();
            }
        }
        drop(socket);
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(300), listener.accept())
            .await
            .is_err(),
        "unexpected upstream submission"
    );
}

#[tokio::test]
async fn gateway_http_stream_header_budgets_and_replay() {
    if isolated("regressions::gateway_http_stream_header_budgets_and_replay").await {
        return;
    }
    let state = crate::paths::state_dir_override().unwrap();
    crate::request_log::set_request_log_path_for_tests(Some(state.join("request-log.jsonl")));
    crate::diagnostic_log::set_diagnostic_log_path_for_tests(Some(state.join("codex-plus.log")));
    let success = HttpFixtureResponse {
        status: "200 OK",
        header_delay_ms: 0,
        output_delay_ms: 0,
        truncate_stream: false,
        close_before_headers: false,
    };
    let unavailable = HttpFixtureResponse {
        status: "503 Service Unavailable",
        ..success
    };
    let redirect = HttpFixtureResponse {
        status: "307 Temporary Redirect",
        ..success
    };
    let slow_header = HttpFixtureResponse {
        header_delay_ms: 1_250,
        ..success
    };
    let slow_output = HttpFixtureResponse {
        output_delay_ms: 1_250,
        ..success
    };
    let truncated = HttpFixtureResponse {
        truncate_stream: true,
        ..success
    };
    let header_timeout = Some("gateway_upstream_header_timeout");
    for (case, model, provider_secs, total_secs, first, second, expected_code) in [
        ("provider", "gpt-6.2-sol", 1, 4, slow_header, None, header_timeout),
        ("disconnect", "gpt-6.2-sol", 2, 4, HttpFixtureResponse { close_before_headers: true, ..success }, None, Some("gateway_upstream_outcome_unknown")),
        ("first-output", "gpt-6.2-astra", 1, 1, slow_output, None, None),
        ("override", "gpt-6.2-sol", 3, 4, slow_header, None, None),
        ("failover", "gpt-6.2-astra", 2, 4, unavailable, Some(success), None),
        (
            "total",
            "gpt-6.2-astra",
            4,
            1,
            HttpFixtureResponse { header_delay_ms: 550, ..unavailable },
            Some(HttpFixtureResponse { header_delay_ms: 800, ..success }),
            header_timeout,
        ),
        ("stream", "gpt-6.2-sol", 2, 4, truncated, None, None),
        ("redirect", "gpt-6.2-sol", 2, 4, redirect, None, Some("gateway_upstream_redirect")),
    ] {
        let primary = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let backup = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut primary_relay = profile(primary.local_addr().unwrap());
        let mut backup_relay = profile(backup.local_addr().unwrap());
        let primary_id = format!("header-primary-{case}");
        let backup_id = format!("header-backup-{case}");
        for (relay, id) in [(&mut primary_relay, &primary_id), (&mut backup_relay, &backup_id)] {
            relay["id"] = json!(id);
            relay["modelList"] = json!(model);
        }
        let fixture = Fixture::new(primary_relay.clone());
        fixture.gateway_profiles(vec![primary_relay, backup_relay]);
        let store = GatewayStore::default();
        let mut gateway = store.load().unwrap();
        gateway.groups.push(
            serde_json::from_value(json!({
                "id": "header-test", "name": "Header test", "sticky": "off",
                "members": [
                    {"model": format!("{primary_id}/{model}")},
                    {"model": format!("{backup_id}/{model}")}
                ]
            }))
            .unwrap(),
        );
        store.save(&gateway).unwrap();
        std::fs::write(
            fixture.temp.path().join("settings.json"),
            json!({
                "gatewayStreamHeaderBudgetSecs": total_secs,
                "gatewayProviderStreamHeaderTimeoutSecs": { (primary_id.as_str()): provider_secs }
            })
            .to_string(),
        )
        .unwrap();

        let redirect = (case == "redirect").then(|| backup.local_addr().unwrap());
        let primary_task = tokio::spawn(scheduled_http_upstream(
            primary,
            Some(first),
            model,
            case == "provider",
            redirect,
        ));
        let backup_task = tokio::spawn(scheduled_http_upstream(backup, second, model, case == "total", None));
        let (url, task) = helper(1).await;
        let mut response = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .post(format!("{}/gw/codex/v1/responses", url.replacen("ws://", "http://", 1)))
            .bearer_auth("local-test")
            .json(&json!({"model":"group/header-test","input":"fixture", "stream":true,"store":false}))
            .send()
            .await
            .unwrap();
        if let Some(code) = expected_code {
            assert_eq!(response.status(), 400, "case {case}");
            let body: Value = response.json().await.unwrap();
            assert_eq!(body["error"]["type"], "gateway_error");
            assert_eq!(body["error"]["code"], code);
        } else if case == "stream" {
            assert_eq!(response.status(), 200);
            let mut body = Vec::new();
            while let Ok(Some(chunk)) = response.chunk().await {
                body.extend_from_slice(&chunk);
            }
            let body = String::from_utf8(body).unwrap();
            assert!(body.contains("fixture-output"));
            assert!(!body.contains("response.completed"));
        } else {
            assert_eq!(response.status(), 200, "case {case}");
            let body = response.text().await.unwrap();
            assert!(body.contains("response.completed"), "case {case}");
        }
        finish(task).await;
        finish(primary_task).await;
        finish(backup_task).await;

        let records = std::fs::read_to_string(crate::request_log::request_log_path()).unwrap();
        let request: Value = serde_json::from_str(records.lines().last().unwrap()).unwrap();
        let diagnostic =
            std::fs::read_to_string(crate::diagnostic_log::diagnostic_log_path()).unwrap();
        let events: Vec<_> = diagnostic
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter(|event| event["detail"]["requestId"] == request["id"])
            .collect();
        let attempts: Vec<_> = events
            .iter()
            .filter(|event| event["event"] == "gateway.http_upstream_attempt")
            .map(|event| &event["detail"])
            .collect();
        assert_eq!(attempts.len(), 1 + usize::from(second.is_some()), "case {case}");
        for (index, detail) in attempts.iter().enumerate() {
            assert_eq!(detail["candidate"], index + 1);
            assert_eq!(detail["attempt"], 1);
            assert_eq!(detail["upstreamModel"], model);
            assert_eq!(detail["queueWaitMs"], 0);
            assert!(detail["bodyBytes"].as_u64().unwrap() > 0);
        }
        let failures: Vec<_> = events
            .iter()
            .filter(|event| event["event"] == "gateway.upstream_failed")
            .map(|event| &event["detail"])
            .collect();
        if second.is_some() {
            assert_eq!(failures[0]["candidate"], 1);
            assert_eq!(failures[0]["willFailover"], true);
        }
        let last = attempts.last().unwrap();
        assert_eq!(last.get("transportError").is_some(), case == "disconnect");
        if let Some(code) = expected_code {
            if case == "redirect" {
                assert_eq!(last["result"], "redirect_refused");
                assert_eq!(last["statusCode"], 307);
            } else if case == "disconnect" {
                assert_eq!(last["result"], "disconnected_before_headers");
                assert_eq!(last["retry"], false);
                assert!(!last["transportError"]["causes"].as_array().unwrap().is_empty());
                println!("pre-header close diagnostic: {last}");
            } else {
                assert_eq!(last["result"], "header_timeout");
            }
            let failure = failures.last().unwrap();
            assert_eq!(failure["class"], code);
            assert_eq!(failure["willFailover"], false);
            assert_eq!(request["statusCode"], 400);
            assert!(request["firstTokenMs"].is_null());
            assert_eq!(request["attempts"], attempts.len());
            if case == "total" {
                assert_eq!(attempts[0]["statusCode"], 503);
                assert!(last["headerBudgetMs"].as_u64().unwrap() < 650);
            }
        } else if case == "first-output" {
            assert_eq!(last["result"], "headers_received");
            assert!(request["firstTokenMs"].as_u64().unwrap() >= 1_000);
        } else if case == "stream" {
            assert_eq!(last["result"], "headers_received");
            assert_eq!(request["success"], false);
            assert!(request["streamOutcome"].as_str().is_some());
            assert!(request["finalResponseId"].is_null());
        }
    }
}

#[tokio::test]
async fn gateway_http_stream_budget_caps_local_queue_without_submission() {
    if isolated("regressions::gateway_http_stream_budget_caps_local_queue_without_submission").await {
        return;
    }
    let state = crate::paths::state_dir_override().unwrap();
    crate::request_log::set_request_log_path_for_tests(Some(state.join("request-log.jsonl")));
    crate::diagnostic_log::set_diagnostic_log_path_for_tests(Some(state.join("codex-plus.log")));
    let primary = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let relay = profile(primary.local_addr().unwrap());
    let fixture = Fixture::new(relay.clone());
    fixture.gateway(relay);
    std::fs::write(
        fixture.temp.path().join("settings.json"),
        r#"{"gatewayStreamHeaderBudgetSecs":1}"#,
    )
    .unwrap();
    let (occupied_sender, occupied_receiver) = tokio::sync::oneshot::channel();
    let (release_sender, release_receiver) = tokio::sync::oneshot::channel();
    let upstream = tokio::spawn(async move {
        let (mut socket, _) = primary.accept().await.unwrap();
        assert_eq!(http_fixture_body(&mut socket).await["input"], "occupy slot");
        let delta = "data: {\"type\":\"response.output_text.delta\",\"delta\":\"started\"}\n\n";
        let terminal = format!("data: {}\n\n", completed("fixture", ""));
        let headers = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            delta.len() + terminal.len(),
        );
        socket.write_all(headers.as_bytes()).await.unwrap();
        socket.write_all(delta.as_bytes()).await.unwrap();
        occupied_sender.send(()).unwrap();
        tokio::select! {
            release = release_receiver => release.unwrap(),
            extra = primary.accept() => panic!("queued request reached upstream: {extra:?}"),
        }
        socket.write_all(terminal.as_bytes()).await.unwrap();
    });
    let (url, helper_task) = helper(2).await;
    let client = reqwest::Client::builder().timeout(Duration::from_secs(4)).no_proxy().build().unwrap();
    let url = format!("{}/gw/codex/v1/responses", url.replacen("ws://", "http://", 1));
    let submit = |input| client.post(&url).bearer_auth("local-test").json(&json!({
        "model":"ws-test/ws-model", "input":input, "stream":true, "store":false
    }));
    let busy = tokio::spawn(submit("occupy slot").send());
    tokio::time::timeout(Duration::from_secs(3), occupied_receiver).await.unwrap().unwrap();
    let response = submit("wait locally").send().await.unwrap();
    assert_eq!(response.status(), 400);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["error"]["code"], "gateway_header_budget_exhausted");
    release_sender.send(()).unwrap();
    let busy_response = busy.await.unwrap().unwrap().text().await.unwrap();
    assert!(busy_response.contains("response.completed"));
    finish(upstream).await;
    finish(helper_task).await;

    let diagnostic = std::fs::read_to_string(crate::diagnostic_log::diagnostic_log_path()).unwrap();
    let events: Vec<Value> = diagnostic.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    let exhausted = events.iter().find(|event| event["event"] == "gateway.http_header_budget_exhausted").unwrap();
    assert_eq!(exhausted["detail"]["phase"], "local_queue");
    assert!(exhausted["detail"]["queueWaitMs"].as_u64().unwrap() >= 900);
    let sends: Vec<_> = events.iter().filter(|event| event["event"] == "gateway.http_upstream_attempt").collect();
    assert_eq!(sends.len(), 1);
    assert_ne!(sends[0]["detail"]["requestId"], exhausted["detail"]["requestId"]);
}

async fn http_fixture_body(socket: &mut tokio::net::TcpStream) -> Value {
    let mut reader = BufReader::new(socket);
    let mut line = String::new();
    let mut length = None;
    loop {
        line.clear();
        assert_ne!(reader.read_line(&mut line).await.unwrap(), 0);
        if line == "\r\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            length = Some(value.trim().parse::<usize>().unwrap());
        }
    }
    let mut bytes = vec![0; length.unwrap()];
    reader.read_exact(&mut bytes).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn official_gateway_preserves_two_incremental_tool_rounds() {
    if isolated("regressions::official_gateway_preserves_two_incremental_tool_rounds").await {
        return;
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let backup = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    unsafe {
        std::env::set_var(
            "CODEX_PLUS_OFFICIAL_BASE_URL",
            format!("http://{}/v1", listener.local_addr().unwrap()),
        );
    }
    let mut relay = profile(listener.local_addr().unwrap());
    relay["modelList"] = json!("gpt-6.2-sol");
    relay["relayMode"] = json!("official");
    relay["upstreamBaseUrl"] = json!("");
    relay["apiKey"] = json!("");
    relay["apiKeys"] = json!([]);
    relay["activeApiKeyId"] = json!("");
    let auth = json!({
        "auth_mode":"chatgpt","codex_plus_gateway":true,
        "tokens":{"account_id":"test-account","access_token":"test-access",
            "refresh_token":"test-refresh"}
    });
    relay["authContents"] = Value::String(auth.to_string());
    let mut api = profile(backup.local_addr().unwrap());
    api["id"] = json!("ws-api-first");
    api["modelList"] = json!("gpt-6.2-sol");
    let fixture = Fixture::new(relay.clone());
    fixture.gateway_profiles(vec![api, relay]);
    let store = GatewayStore::default();
    let mut settings = store.load().unwrap();
    settings.auto_models = true;
    store.save(&settings).unwrap();
    let upstream = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(socket).await.unwrap();
        let initial = next(&mut ws).await;
        assert_eq!(initial["model"], "gpt-6.2-sol");
        assert_eq!(initial["service_tier"], "priority");
        assert_eq!(
            initial["input"][0]["content"],
            "Inspect the project and continue"
        );
        let cache_key = initial["prompt_cache_key"].as_str().unwrap().to_string();
        assert!(!cache_key.is_empty());
        for round in 0..2 {
            let response_id = format!("resp-{round}");
            // 长调用 ID 必须与上游发出的值一致，不能只在输出回传时哈希。
            let call_id = format!("call-{round}-{}", "c".repeat(80));
            let mut event = completed(&response_id, "");
            event["response"]["output"] = json!([{
                "type":"function_call","id":format!("fc_{round}"),
                "call_id":call_id,"name":"read","arguments":"{}"
            }]);
            send(&mut ws, event).await;
            let continuation = next(&mut ws).await;
            assert_eq!(continuation["model"], "gpt-6.2-sol");
            // Toggle back to ordinary on the first continuation; it must still use the same
            // upstream connection, even though ordinary routing would otherwise prefer the API.
            if round == 0 {
                assert!(continuation.get("service_tier").is_none());
            } else {
                assert_eq!(continuation["service_tier"], "priority");
            }
            assert_eq!(continuation["previous_response_id"], response_id);
            assert_eq!(continuation["prompt_cache_key"], cache_key);
            assert_eq!(continuation["input"].as_array().unwrap().len(), 1);
            assert_eq!(continuation["input"][0]["type"], "function_call_output");
            assert_eq!(continuation["input"][0]["call_id"], call_id);
        }
        send(&mut ws, completed("final", "")).await;
        let _ = ws.next().await;
        assert!(
            tokio::time::timeout(Duration::from_millis(50), listener.accept())
                .await
                .is_err()
        );
    });
    let (url, task) = helper(1).await;
    let mut ws = client(
        &format!("{url}/gw/codex/v1/responses"),
        &[("authorization", "Bearer local-test".into())],
    )
    .await;
    let mut initial = create("");
    initial["model"] = json!("gpt-6.2-sol");
    initial["service_tier"] = json!("priority");
    initial["instructions"] = json!("You are Codex.");
    initial["input"] = json!([{
        "type":"message","role":"user","content":"Inspect the project and continue"
    }]);
    // 故意不提供缓存键，验证首轮生成的键在纯工具增量中仍能复用同一连接。
    send(&mut ws, initial).await;
    for round in 0..2 {
        let response = next(&mut ws).await;
        assert_eq!(response["type"], "response.completed", "{response}");
        let mut continuation = create("");
        continuation["model"] = json!("gpt-6.2-sol");
        if round == 1 {
            continuation["service_tier"] = json!("priority");
        }
        continuation["instructions"] = json!("You are Codex.");
        continuation["previous_response_id"] = response["response"]["id"].clone();
        continuation["input"] = json!([{
            "type":"function_call_output",
            "call_id":format!("call-{round}-{}", "c".repeat(80)),
            "output":format!("listing-{round}")
        }]);
        send(&mut ws, continuation).await;
    }
    let final_response = next(&mut ws).await;
    assert_eq!(
        final_response["response"]["id"], "final",
        "{final_response}"
    );
    ws.close(None).await.unwrap();
    drop(ws);
    finish(task).await;
    finish(upstream).await;
    assert!(tokio::time::timeout(Duration::from_millis(50), backup.accept()).await.is_err());
    unsafe { std::env::remove_var("CODEX_PLUS_OFFICIAL_BASE_URL") };
}

#[tokio::test]
async fn unknown_continuation_is_rejected_without_contacting_upstream() {
    if isolated("regressions::unknown_continuation_is_rejected_without_contacting_upstream").await {
        return;
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let _fixture = Fixture::new(profile(listener.local_addr().unwrap()));
    let (url, task) = helper(1).await;
    let mut ws = client(&format!("{url}/v1/responses"), &[]).await;
    let mut event = create("");
    event["previous_response_id"] = json!("unknown");
    event["input"] = json!([{"type":"function_call_output","call_id":"call","output":"ok"}]);
    send(&mut ws, event).await;
    let error = next(&mut ws).await;
    assert_eq!(error["status"], 400);
    assert_eq!(error["error"]["code"], "previous_response_not_found");
    assert!(
        tokio::time::timeout(Duration::from_millis(50), listener.accept())
            .await
            .is_err()
    );
    ws.close(None).await.unwrap();
    drop(ws);
    finish(task).await;
}

#[tokio::test]
async fn undeclared_responses_upstream_falls_back_before_upgrade() {
    if isolated("regressions::undeclared_responses_upstream_falls_back_before_upgrade").await {
        return;
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut relay = profile(listener.local_addr().unwrap());
    relay["configContents"] = json!("model = \"ws-model\"");
    let fixture = Fixture::new(relay.clone());
    fixture.gateway(relay);
    let (url, task) = helper(1).await;
    assert_eq!(
        upgrade_status(&format!("{url}/v1/responses")).await,
        "HTTP/1.1 426 Upgrade Required"
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(50), listener.accept())
            .await
            .is_err()
    );
    finish(task).await;
}

#[test]
fn websocket_capability_uses_only_the_active_provider_declaration() {
    let mut relay: crate::settings::RelayProfile =
        serde_json::from_value(profile("127.0.0.1:1".parse().unwrap())).unwrap();
    assert!(crate::protocol_proxy::relay_supports_responses_websocket(
        &relay
    ));
    relay.config_contents = "model_provider = 'http'\n[model_providers.http]\nsupports_websockets = false\n[model_providers.unused]\nsupports_websockets = true".into();
    assert!(!crate::protocol_proxy::relay_supports_responses_websocket(
        &relay
    ));
    relay.config_contents = "invalid TOML [".into();
    assert!(!crate::protocol_proxy::relay_supports_responses_websocket(
        &relay
    ));
    relay.config_contents =
        "model_provider='ws'\n[model_providers.ws]\nsupports_websockets=true".into();
    relay.protocol = crate::settings::RelayProtocol::ChatCompletions;
    assert!(!crate::protocol_proxy::relay_supports_responses_websocket(
        &relay
    ));
}
