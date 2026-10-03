# Windows 启动前运行时检查（可选）

问题：Codex 更新后的兼容运行时仍在修复时，管理器直接重启 Codex，会中断修复或让新进程加载旧运行时。等待完整检查后再激活应用，可封住管理器启动和重启的共同入口。

`DefaultLaunchHooks::launch_codex` 在启动应用前读取启动器旁的
`codex-plus-plus.runtime-gate.json`。没有此文件时保留原有行为。配置存在但无效、脚本失败或超过 300 秒时，启动失败。外部脚本输出不进入应用诊断日志。

示例（路径应取目标机器上的实际值）：

```json
{
  "schema_version": 1,
  "powershell": "C:\\Program Files\\PowerShell\\7\\pwsh.exe",
  "script": "D:\\CodexContextMenu\\Ensure-CodexLaunchReady.ps1"
}
```

脚本参数为 `-ExpectedAppDirectory <resolved app directory>`，通过参数数组调用。
脚本应检查当前安装版本、完整运行时、代理及实际选中的目录；修复失败时退出非零，不能自行再次启动 Codex++。

新增 `--check-runtime-only` 供部署验收：解析应用位置并执行同一检查，不启动管理器、助手或 Codex，也不激活已有窗口。

Windows 构建机应使用已有 MSVC / Windows SDK 环境：

```powershell
$env:CODEX_GATE_TEST_POWERSHELL = (Get-Command pwsh.exe).Source
cargo test -p codex-plus-core --lib runtime_gate --locked
cargo build -p codex-plus-launcher --release --locked
```

测试覆盖缺少配置、损坏配置、含空格和 shell 特殊字符的路径、检查失败和超时停止。产物是 `target/release/codex-plus-plus.exe`。仅需更换启动器，管理器保持原版本。

本机兼容层还将代理运行时直接构建到 `D:\CodexContextMenu\CuaRuntimeStore`，验证成功后再在 App 管理目录注册 Junction。构建源来自当前安装包，避免 App 清理管理目录时删除尚未注册的候选运行时。

此源代码改动必须保留在后续 Codex++ 构建中。直接安装未包含本补丁的发布版会失去启动检查；JSON 文件本身不能给未修改的程序增加此功能。

配套兼容层脚本与隔离验证脚本保存在 `scripts/windows-runtime-readiness/`。
这些是现有本机兼容层的补丁与测试，完整依赖见同目录 README；启用前应确认目标
机器的检查脚本和其修复链已安装。
