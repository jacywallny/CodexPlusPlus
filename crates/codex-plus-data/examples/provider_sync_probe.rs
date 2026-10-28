//! 只允许在显式标记的隔离副本验证供应商同步，拒绝正式 CODEX_HOME。
use std::path::PathBuf;

fn main() {
    let mut args = std::env::args_os().skip(1);
    let home = PathBuf::from(args.next().expect("usage: provider_sync_probe <isolated-home>"));
    assert!(args.next().is_none(), "unexpected arguments");
    assert!(home.join(".provider-sync-validation-copy").is_file(), "unmarked validation home");
    let home = std::fs::canonicalize(home).expect("validation home must exist");
    let result = codex_plus_data::run_provider_sync(Some(&home));
    let routing = codex_plus_data::validate_provider_sync_routing(Some(&home));
    let routing_consistent = routing.is_ok();
    println!("{}", serde_json::json!({"result":result,"routingConsistent":routing_consistent,"routingError":routing.err().map(|e|e.to_string())}));
    if result.status != codex_plus_data::ProviderSyncStatus::Synced || !routing_consistent {
        std::process::exit(1);
    }
}
