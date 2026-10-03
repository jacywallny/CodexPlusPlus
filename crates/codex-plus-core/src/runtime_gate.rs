//! 可选的本机启动检查。放在应用激活之前，管理器启动和重启共用这一入口。
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use tokio::process::Command;

const CONFIG_NAME: &str = "codex-plus-plus.runtime-gate.json";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GateConfig {
    schema_version: u32,
    powershell: PathBuf,
    script: PathBuf,
}

fn load_config(directory: &Path) -> Result<Option<GateConfig>> {
    let filename = directory.join(CONFIG_NAME);
    let bytes = match std::fs::read(&filename) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("无法读取 Codex 启动检查配置"),
    };
    let config: GateConfig =
        serde_json::from_slice(&bytes).context("Codex 启动检查配置无效；未启动应用")?;
    if config.schema_version != 1 {
        bail!("不支持的 Codex 启动检查配置版本；未启动应用");
    }
    for path in [&config.powershell, &config.script] {
        if !path.is_absolute() || !path.is_file() {
            bail!("Codex 启动检查文件不存在或不是绝对路径：{}", path.display());
        }
    }
    Ok(Some(config))
}

async fn run_gate(config: &GateConfig, app_dir: &Path, timeout: Duration) -> Result<()> {
    let mut command = Command::new(&config.powershell);
    command
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-File"])
        .arg(&config.script)
        .arg("-ExpectedAppDirectory")
        .arg(app_dir)
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(crate::windows_integration::CREATE_NO_WINDOW);
    let output = tokio::time::timeout(timeout, command.output())
        .await
        .context("Codex 运行时检查超时；未启动应用，请查看本机修复日志")?
        .context("无法执行 Codex 运行时检查；未启动应用")?;
    if !output.status.success() {
        // 不转发外部脚本输出，避免把配置或凭据带入界面与诊断日志。
        bail!(
            "Codex 运行时或代理尚未就绪；未启动应用（检查退出码 {:?}），请查看本机修复日志",
            output.status.code()
        );
    }
    Ok(())
}

pub async fn ensure_windows_runtime_ready(app_dir: &Path) -> Result<()> {
    let executable = std::env::current_exe().context("无法定位 Codex++ 启动器")?;
    let directory = executable.parent().context("Codex++ 启动器目录不存在")?;
    if let Some(config) = load_config(directory)? {
        let _ = crate::diagnostic_log::append_diagnostic_log(
            "launcher.runtime_check_started",
            serde_json::json!({"app_dir": app_dir}),
        );
        run_gate(&config, app_dir, Duration::from_secs(300)).await?;
        let _ = crate::diagnostic_log::append_diagnostic_log(
            "launcher.runtime_check_passed",
            serde_json::json!({"app_dir": app_dir}),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_config_preserves_standard_launch() {
        let directory = tempfile::tempdir().unwrap();
        assert!(load_config(directory.path()).unwrap().is_none());
    }

    #[test]
    fn configured_but_broken_gate_cannot_be_silently_skipped() {
        let directory = tempfile::tempdir().unwrap();
        let config = directory.path().join(CONFIG_NAME);
        std::fs::write(&config, "{").unwrap();
        assert!(load_config(directory.path()).is_err());
        std::fs::write(
            &config,
            r#"{"schema_version":1,"powershell":"pwsh.exe","script":"gate.ps1"}"#,
        )
        .unwrap();
        assert!(load_config(directory.path()).is_err());
    }

    #[cfg(windows)]
    fn fixture(script_text: &str) -> (tempfile::TempDir, GateConfig) {
        let directory = tempfile::tempdir().unwrap();
        let script = directory.path().join("检查 with spaces.ps1");
        std::fs::write(&script, script_text).unwrap();
        // 测试沿用运行 cargo 的 shell 已解析的 PowerShell，可通过环境指定绝对路径。
        let powershell = std::env::var_os("CODEX_GATE_TEST_POWERSHELL")
            .map(PathBuf::from)
            .expect("set CODEX_GATE_TEST_POWERSHELL to the installed pwsh.exe");
        let config = GateConfig {
            schema_version: 1,
            powershell,
            script,
        };
        (directory, config)
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn spaces_and_shell_metacharacters_are_literal_arguments() {
        let (directory, config) = fixture(
            "param([string]$ExpectedAppDirectory)\n[IO.File]::WriteAllText((Join-Path $PSScriptRoot 'received.txt'), $ExpectedAppDirectory)\nexit 0",
        );
        let app = directory.path().join("app with spaces & $literal");
        run_gate(&config, &app, Duration::from_secs(10))
            .await
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(directory.path().join("received.txt")).unwrap(),
            app.to_string_lossy()
        );
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn failed_check_prevents_launch() {
        let (_directory, config) = fixture("param([string]$ExpectedAppDirectory)\nexit 7");
        let result = run_gate(&config, Path::new("C:\\app"), Duration::from_secs(10)).await;
        assert!(result.unwrap_err().to_string().contains("退出码 Some(7)"));
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn timed_out_check_is_stopped() {
        let (directory, config) = fixture(
            "param([string]$ExpectedAppDirectory)\nStart-Sleep -Seconds 2\nSet-Content -LiteralPath (Join-Path $PSScriptRoot 'must-not-exist.txt') -Value 'late'",
        );
        assert!(
            run_gate(&config, Path::new("C:\\app"), Duration::from_millis(200))
                .await
                .is_err()
        );
        tokio::time::sleep(Duration::from_secs(3)).await;
        assert!(!directory.path().join("must-not-exist.txt").exists());
    }
}
