# Windows 兼容层脚本快照

这里保存启动检查与稳定存储迁移的实现、测试，避免只保留在机器本地。

`Ensure-CodexLaunchReady.ps1` 是本机已有 Codex 兼容层的配套脚本。它要求同目录
已有 `CodexRuntimeReadiness.ps1` 和 `Repair-CodexAfterPackageUpdate.ps1` 及该修复链
依赖。`Repair-CodexCuaRuntime.ps1` 要求已有可信的 NodeReplProxyLauncher。
这些文件不是独立安装器；不具备现有兼容层时，请先提供适合目标机器的检查脚本，
再启用启动器旁的 JSON 配置。缺少依赖时检查会阻止启动。

`Test-StableProxyRegistration.ps1` 默认测试同目录的修复脚本，可通过
`-ProductionScript` 指定实际安装脚本。它使用隔离 fixture 与模拟进程，不修改
活动运行时；fixture 为便于失败排查而保留。

`Test-LauncherRuntimeGate.ps1 -LauncherPath <exe>` 使用独立 EXE 副本，检查
脚本失败、脚本成功、配置损坏三种状态，确认没有启动或重启桌面进程。

编译、开关和部署说明见 `docs/windows-runtime-readiness.md`。
