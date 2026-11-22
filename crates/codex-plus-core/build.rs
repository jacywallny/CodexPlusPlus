fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let version = env!("CARGO_PKG_VERSION");
    let public_version = match version.split_once('+') {
        Some((base, revision)) if !base.contains('-') && revision.parse::<u16>().is_ok() => {
            format!("{base}.{revision}")
        }
        _ => version.to_owned(),
    };
    println!("cargo:rustc-env=CODEX_PLUS_PUBLIC_VERSION={public_version}");
}
