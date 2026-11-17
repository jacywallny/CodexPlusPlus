use super::*;

#[tokio::test]
async fn official_gateway_preserves_two_incremental_tool_rounds() {
    if isolated("regressions::official_gateway_preserves_two_incremental_tool_rounds").await {
        return;
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    unsafe {
        std::env::set_var(
            "CODEX_PLUS_OFFICIAL_BASE_URL",
            format!("http://{}/v1", listener.local_addr().unwrap()),
        );
    }
    let mut relay = profile(listener.local_addr().unwrap());
    relay["relayMode"] = json!("official");
    relay["upstreamBaseUrl"] = json!("");
    relay["apiKey"] = json!("");
    relay["apiKeys"] = json!([]);
    relay["activeApiKeyId"] = json!("");
    relay["authContents"] = json!({
        "auth_mode":"chatgpt","codex_plus_gateway":true,
        "tokens":{"account_id":"test-account","access_token":"test-access",
            "refresh_token":"test-refresh"}
    }.to_string());
    let fixture = Fixture::new(relay.clone());
    fixture.gateway(relay);
    let upstream = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(socket).await.unwrap();
        let initial = next(&mut ws).await;
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
    assert_eq!(next(&mut ws).await["status"], 400);
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
