pub const VERSION: &str = env!("CODEX_PLUS_PUBLIC_VERSION");

#[cfg(test)]
mod tests {
    use super::VERSION;

    #[test]
    fn exposes_workspace_version() {
        let package_version = env!("CARGO_PKG_VERSION");
        if let Some((base, metadata)) = package_version.split_once('+')
            && !base.contains('-')
            && let Ok(revision) = metadata.parse::<u16>()
        {
            assert_eq!(VERSION, format!("{base}.{revision}"));
        } else {
            assert_eq!(VERSION, package_version);
        }
    }
}
