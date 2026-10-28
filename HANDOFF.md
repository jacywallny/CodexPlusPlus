# Codex++ 1.12.2 融合与构建索引

更新时间：2026-10-28 13:38（Asia/Shanghai）

## 目标与边界

- 以官方 `v1.12.1` 为基线，保留本机已提交的修复，并把本次融合版本号定为 `1.12.2`。
- 不引入 2.0 网关主线；仅在 Jacy 电脑构建验证和生成产物，不安装、不替换当前使用的 2.0。
- 2.0 的 Git 备份位于 `D:\codex_cli\backups\codex-plus-plus-2.0.0-preserved-20261028`，提交 `eba792a`，标签 `v2.0.0-preserved-20261028`。

## 源码与融合基线

- 源码根目录：`D:\github\CodexPlusPlus-upstream-integration-20261028`
- 分支：`codex/integrate-upstream-1.12.1-20261028`
- 官方基线：标签 `v1.12.1`，提交 `ca1b212db8c2baca87fc2bd5b7fad460c437159c`
- 原来源机器的最终提交：`67e492a06e0e4689b33152c4c6bd2d1775ce1171`。
- Jacy 上的源码快照目录：`D:\github\CodexPlusPlus-upstream-integration-20261028-build`。该目录从来源机器的 Git 归档建立，接替后补了下述三项融合修正；不能再将当前源码描述为与 `67e492a0` 完全相同。
- Jacy 最终通过构建验证的源码提交：`f96eb2bcd3e973e1d43b552865085b856c42c3b2`。后续仅更新交接文档的提交不改变此构建溯源编号。
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
- 构建产物与校验值已在 Jacy 本机生成；尚未传回原来源机器。

## 当前进度

- 构建完成：2026-10-28 13:37:15（Asia/Shanghai），`build-validation\1.12.2\status.json` 的阶段为 `complete`，9 个阶段退出码均为 0。
- 接替后修正：`2a0befe` 将配置重读断言改为比较 TOML 语义并保持其他字段完整相等检查；`716eddc` 删除 `backup.rs` 中等价的重复 `sync_file` 定义，保留可写句柄刷盘实现；`f96eb2b` 给两项压缩会话目录测试补齐初始供应商参数。
- 测试通过：核心库及所选集成测试 1626 项、数据层 312 项、启动器 39 项、前端 825 项，共 2802 项通过，0 项失败；前端类型检查和 Vite 构建也已通过。
- 产物通过：Release 主程序与 provider 同步探针均已编译，主程序为 Windows x64，PE 文件版本和产品版本均为 `1.12.2`；ZIP 的两个条目已逐项读取并核对 SHA-256，与独立 EXE 一致。
- Jacy 当前安装路径 `D:\Codex++\codex-plus-plus.exe` 的文件版本和产品版本均为 `1.12.1`；原来源机器的 2.0 安装本轮未检查。此次仅构建产物，没有安装、启动或部署。
- 前两轮失败日志和修改前源码已保留在 `build-validation\resume-backup-*`、`build-validation\data-compile-failure-*`、`build-validation\fixture-compile-failure-*`；原目录的旧交接索引也已单独备份。
- 继续接替时先读本文件及 `build-validation\1.12.2\status.json`。Jacy 上不带 `-build` 的同名目录是早期 `07f0f14` 快照，不能用它替代本次最终构建源码。需要传回来源机器的是构建产物、校验记录及三项修正。
- `git diff --check` 通过。全仓 `cargo fmt --all -- --check` 会报告大量仓库既有文件格式差异；不要对全仓执行自动格式化，以免引入无关改动。
- 不把 Jacy 构建产物复制到 `D:\Codex++\codex-plus-plus.exe`，不执行安装或部署。

## 预设忽略测试的边界

以下 7 项由源码预先标记，未因本轮失败临时跳过；缺少对应真实环境或显式样本，因此不能声称相关真实环境功能已验收。

- 核心库 4 项：`real_chromium_file_response_is_patched_and_opt_out_restores_gates` 需要独立 Chromium 页面；`pinned_fixture_transaction_recovery_and_external_change` 需要指定运行时样本和描述文件；`structural_fixture_transaction_recovery_and_component_drift` 需要指定结构样本；`live_native_browser_connection` 需要正在运行的 Chrome/Edge 原生扩展。
- 数据层 2 项：`measure_provider_scan_read_only` 需要 `CPP_PROVIDER_SCAN_HOME`，用于只读耗时测量；`verify_real_rollout_read_only` 需要指定真实会话、回合及文本样本。
- 前端 1 项：`real local service fixtures retain their structural contract` 需要 `CPP_BROWSER_SERVICE_FIXTURES`，至少提供三个本机服务样本。

## 成品校验

目录：`dist\jacy-build\1.12.2`。校验文本：`SHA256SUMS.txt`。

| 文件 | 字节数 | SHA-256 |
| --- | ---: | --- |
| `codex-plus-plus.exe` | 79297536 | `0A137798B8CFE954E00DEF7A58AEF18AA2A3F9FF9B4D59B9D446054E437757FE` |
| `provider_sync_probe.exe` | 22017024 | `E620689B33B62D0DDB32EAD724C1715A597F270F45A75C0AC90B87DEB81A4237` |
| `CodexPlusPlus-1.12.2-windows-x64.zip` | 42068029 | `7A5FDFA7E830D0C43EE527221A10DBB4C7195E685D45F0FA1651E7261E0AFB3F` |
