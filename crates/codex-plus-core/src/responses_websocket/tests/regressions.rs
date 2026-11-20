use super::*;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

#[tokio::test]
async fn gateway_http_official_pre_output_failover() {
    if isolated("regressions::gateway_http_official_pre_output_failover").await {
        return;
    }
    // The long fixture exceeds 270k estimated tokens; neither request reaches a real provider.
    for (case, model, length, first_event, expected_backup) in [
        (0, "gpt-6.2-sol", 24, None, true),
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
        assert_eq!(response.status(), 200, "case {case}");
        if expected_backup {
            let body = response.text().await.unwrap();
            assert!(body.contains("backup-output"), "case {case}: {body}");
            assert!(!body.contains("response.created"));
        } else {
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
