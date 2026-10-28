# Codex++ 1.12.2 融合与构建索引

更新时间：2026-10-28 13:00（Asia/Shanghai）

## 目标与边界

- 以官方 `v1.12.1` 为基线，保留本机已提交的修复，并把本次融合版本号定为 `1.12.2`。
- 不引入 2.0 网关主线；仅在 Jacy 电脑构建验证和生成产物，不安装、不替换当前使用的 2.0。
- 2.0 的 Git 备份位于 `D:\codex_cli\backups\codex-plus-plus-2.0.0-preserved-20261028`，提交 `eba792a`，标签 `v2.0.0-preserved-20261028`。

## 源码与融合基线

- 源码根目录：`D:\github\CodexPlusPlus-upstream-integration-20261028`
- 分支：`codex/integrate-upstream-1.12.1-20261028`
- 官方基线：标签 `v1.12.1`，提交 `ca1b212db8c2baca87fc2bd5b7fad460c437159c`
- 当前本机最终提交：`67e492a06e0e4689b33152c4c6bd2d1775ce1171`。
- Jacy 上的源码快照目录：`D:\github\CodexPlusPlus-upstream-integration-20261028-build`。该目录是从本机 Git 归档解压后建立的干净 Git 快照，远端本地快照提交会与本机提交号不同，但源码内容对应 `67e492a0`。
- 分支已有本地会话压缩、provider 同步/切换、路由校验、锁处理和 Windows 构建修复；本次工作树另有 1.12.2 版本字段、运行时供应商切换需确认重启的实现及构建脚本调整。
- 上游仓库自带的 `HANDOVER.md` 记录其他上游 PR，不是本次融合任务的状态索引；本文件只记录本次工作。

## Jacy 构建入口

- Jacy SSH 别名：`jacy`；使用 `C:\Program Files\OpenSSH\ssh.exe`。连接检测脚本：`D:\Tools\jacy_connection_watchdog\connect-jacy.ps1 -TestOnly`。
- Jacy 的 VS Build Tools、Rust、Node.js 和共享 Cargo target 缓存已确认存在；旧融合目录 `D:\github\CodexPlusPlus-upstream-integration-20261022` 是 1.11.1，不能直接用作本次源码目录。
- 将最终源码快照传到 Jacy 的目录 `D:\github\CodexPlusPlus-upstream-integration-20261028-build`，保持 Git 工作树干净，然后在 Jacy 运行：

  ```powershell
  Set-Location 'D:\github\CodexPlusPlus-upstream-integration-20261028-build'
  .\scripts\build-windows-jacy.ps1 -SourceRoot 'D:\github\CodexPlusPlus-upstream-integration-20261028-build' -ExpectedCommit '<Jacy 快照提交>' -Version '1.12.2'
  ```

- 构建脚本：`scripts\build-windows-jacy.ps1`。它检查指定提交、版本字段和 MSVC linker，运行 Rust 测试、前端安装/检查/测试/构建、release 构建和 provider 同步探针构建，并记录阶段日志。
- Jacy 产物与验证记录：`D:\github\CodexPlusPlus-upstream-integration-20261028-build\dist\jacy-build\1.12.2`；状态/日志在 `build-validation\1.12.2`。
- 构建成功后将 ZIP 与 SHA-256、PE 文件/产品版本核验结果同步回本机，并更新本索引。

## 当前进度

- 已验证：工作分支以官方 `v1.12.1` 为祖先；版本字段已改为 `1.12.2`；2.0 备份提交与标签存在；Jacy SSH 连通且工具链目录存在；本机安装文件仍为 2.0.0。
- 已完成：本机最终提交为 `67e492a0`；Jacy 已成功编译并运行核心测试，当前快照中除旧断言外的核心测试均通过；测试修正已改为检查压缩配置语义，避免 Windows TOML 空行差异。
- 当前阻塞：最新修正快照尚未因 SSH/SCP 远端文件句柄问题同步到 Jacy；上一轮构建只剩一个旧的配置字符串断言失败，不能作为最终构建结论。
- Jacy Codex 接替方法：在 Jacy 上打开 `D:\github\CodexPlusPlus-upstream-integration-20261028-build`，先读本文件，再检查本机最终提交 `67e492a0` 对应的源码快照，清理/复用 `build-validation\1.12.2` 后重新运行上面的构建命令。继续时优先使用 Jacy 本机文件复制或唯一新文件名，避免覆盖正在被 `scp` 占用的压缩包。
- `git diff --check` 通过。全仓 `cargo fmt --all -- --check` 会报告大量仓库既有文件格式差异；不要对全仓执行自动格式化，以免引入无关改动。
- 不把 Jacy 构建产物复制到 `D:\Codex++\codex-plus-plus.exe`，不执行安装或部署。
