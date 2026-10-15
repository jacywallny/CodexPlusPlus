//! Run provider sync only against an explicitly marked, isolated validation copy.
use std::path::PathBuf;

fn main() {
    let mut args = std::env::args_os().skip(1);
    let home = PathBuf::from(args.next().expect("usage: provider_sync_probe <isolated-home> [provider]"));
    assert!(home.join(".provider-sync-validation-copy").is_file(), "refusing an unmarked home; create an isolated validation copy first");
    let home = std::fs::canonicalize(home).expect("validation home must exist");
    let provider = args.next().map(|value| value.into_string().expect("provider must be UTF-8"));
    assert!(args.next().is_none(), "unexpected arguments");
    let started = std::time::Instant::now();
    let result = codex_plus_data::run_provider_sync_with_target(Some(&home), provider.as_deref());
    println!("{}", serde_json::json!({"elapsedMs":started.elapsed().as_millis(),"result":result}));
    if result.status != codex_plus_data::ProviderSyncStatus::Synced {
        std::process::exit(1);
    }
}
