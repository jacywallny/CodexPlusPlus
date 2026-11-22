use codex_plus_core::settings::SettingsStore;
use serde_json::json;

#[test]
fn auto_group_circuit_breaker_updates_only_its_setting() {
    let temporary = tempfile::tempdir().unwrap();
    let settings_path = temporary.path().join("settings.json");
    std::fs::write(
        &settings_path,
        json!({
            "gatewayProviderStreamHeaderTimeoutSecs": {"fixture": 180},
            "gatewayStreamFirstOutputTimeoutSecs": 30,
            "providerSyncEnabled": false
        })
        .to_string(),
    )
    .unwrap();
    let store = SettingsStore::new(settings_path);
    assert!(!store.load().unwrap().gateway_stream_health_routing_enabled);

    for enabled in [true, false] {
        store
            .update(json!({"gatewayStreamHealthRoutingEnabled": enabled}))
            .unwrap();
        let settings = store.load().unwrap();
        assert_eq!(settings.gateway_stream_health_routing_enabled, enabled);
        assert_eq!(settings.gateway_provider_stream_header_timeout_secs["fixture"], 180);
        assert_eq!(settings.gateway_stream_first_output_timeout_secs, Some(30));
        assert!(!settings.provider_sync_enabled);
    }
}
