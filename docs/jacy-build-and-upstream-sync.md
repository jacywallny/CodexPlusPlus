# 官方更新、个人融合与 Jacy 构建指南

更新时间：2026-10-27（Asia/Shanghai）

本文是 CodexPlusPlus Windows 融合分支的现行操作入口。目标是让官方更新、个人修复和 Jacy 远程编译保持可追溯，并尽量复用已有编译缓存。

## 当前基线

- 官方远端：`origin` → `https://github.com/BigPizzaV3/CodexPlusPlus.git`
- 个人远端：`fork` → `https://github.com/jacywallny/CodexPlusPlus.git`
- 当前已验证构建提交：`19190433764a4c2d7e10e43e14faa181c771e14d`
- 当前源码文档/部署脚本提交：`bcbb098c2e7213ae45d5959a3bdde742f331bc10`
- 当前验证版本：`2.0.0`
- Jacy 源码目录：`D:\github\CodexPlusPlus-rollout-compression-toggle-20261027`
- Jacy 持久 Rust 编译目录：`D:\github\CodexPlusPlus-rollout-compression-toggle-20261027\target-jacy`
- Jacy 构建状态：`build-validation\2.0.0\status.json`

2026-10-27 的 Jacy 验证中，核心、数据、launcher、前端检查、前端测试、前端构建、release 和 probe 均为 `exitCode=0`，最终状态为 `complete`。这证明 `19190433764a4c2d7e10e43e14faa181c771e14d` 在 Jacy 上可编译；后续只增加文档或部署脚本时，不会改变这份二进制验证结果。构建通过不等同于已经安装到本机。

## 分支约定

建议保持以下关系：

```text
origin/main                         官方最新代码
fork/main                           个人 GitHub 主分支
codex/integration                   当前官方与个人代码融合线
codex/<topic>                       一个独立功能或修复
```

每个功能尽量单独提交。不要把多个不相关的修复压成一个大提交，否则后续吸收官方更新时冲突范围会变大。

## 吸收官方更新

在确认工作树干净后执行：

```powershell
git fetch origin
git fetch fork
git switch codex/integration
git merge origin/main
# 将融合提交推到个人 fork，供 Jacy 拉取同一个提交
git push fork HEAD:codex/integration
```

冲突处理原则：

1. 官方实现优先保留协议、上游 API 和新版本结构。
2. 个人实现保留压缩会话兼容、`local_thread_store_compression=false` 设置入口及本地修复测试。
3. 每个冲突解决后先运行相关 Rust 测试，再运行整套 Jacy 验证。
4. 只有验证通过后才推送个人 fork：

```powershell
git push fork codex/integration
```

## 加快 Jacy 编译

- 保留 `target-jacy`，不要每次删除整个目录。
- 只有 Cargo.lock、Rust 工具链、主要依赖或大范围源码变化时才清理对应 target。
- 不要让多个工作树共用旧项目的 target 目录；这会造成测试状态、构建文件和链接缓存互相污染。
- 如果 Jacy 配置了 `sccache`，保持其缓存目录长期存在。
- npm 依赖使用已有 npm cache；只有 `package-lock.json` 变化时才预期大量重新安装。
- 不要在验证脚本中加入 `cargo clean`，除非正在排查缓存污染。

当前脚本已经把 `CARGO_TARGET_DIR` 固定为当前源码根目录下的 `target-jacy`，避免复用其他工作树的构建状态。

## Jacy 完整构建

源码已推到个人 fork 后，在 Jacy 主机执行（`<当前提交>` 必须替换为刚刚推送的完整提交号）：

```powershell
ssh jacy powershell.exe -NoProfile -ExecutionPolicy Bypass -File `
  D:\github\CodexPlusPlus-rollout-compression-toggle-20261027\scripts\build-windows-jacy.ps1 `
  -SourceRoot D:\github\CodexPlusPlus-rollout-compression-toggle-20261027 `
  -ExpectedCommit <当前提交> `
  -Version <版本号>
```

脚本会依次执行：

1. MSVC linker 检查；
2. 核心、数据和 launcher 测试；
3. `npm ci`、前端类型检查、前端测试和 Vite 构建；
4. manager release 构建；
5. `provider_sync_probe` 构建；
6. PE 版本、产物哈希和 ZIP 归档核验。

只有 `status.json` 的 `stage` 为 `complete`，且所有检查的 `exitCode` 都为 `0`，才可以认为该提交在 Jacy 上构建完成。若 Jacy 工作树不是目标提交，先停止部署并重新同步，不能用旧产物冒充新提交。

## 产物与部署边界

产物目录：

```text
D:\github\CodexPlusPlus-rollout-compression-toggle-20261027\dist\jacy-build\<版本号>
```

部署前必须：

- 正常退出旧版 Codex++；
- 备份现有 `D:\Codex++` 程序、runtime-gate 文件和用户设置；
- 核对新程序 SHA-256、PE FileVersion 和 ProductVersion；
- 确认官方 Codex App 进程没有被停止；
- 安装后检查 manager 启动、`providerSyncEnabled` 和 ready 日志。

当前本机 2.0.0 部署脚本入口为：

```text
scripts/deploy-local-2.0.0.ps1
```

先用 `-PrepareOnly` 做只读预检，再执行正式部署；脚本会校验 Jacy 产物版本和 SHA-256、备份旧安装与设置，并写入 `D:\codex_cli\reports`。正式部署后仍需人工确认启动日志和实际设置。

不要把 Jacy 构建通过直接当成本机已安装。部署和构建是两个阶段，安装脚本必须保留可回滚备份。

## 常见失败

- `link.exe` 指向 Git 的 GNU linker：重新进入 VS Build Tools 环境，并确认 `CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER` 指向 MSVC。
- 设置测试出现 `settings_revision_conflict`：检查是否复用了其他工作树的 target，优先使用当前源码根下的 `target-jacy`。
- 前端源码契约因 CRLF 失败：测试读取源码时应统一换行；不要把功能代码改成依赖某一种 checkout 换行格式。
- 远程任务栏原生图标测试失败：先区分 Jacy 远程窗口环境限制和源码失败；只有明确属于环境限制时，才按现有验证脚本的跳过规则处理并保留日志。

## 回滚

发现安装后异常时，使用部署生成的备份目录恢复旧程序和配置；不要删除当前构建目录或验证日志。先保留失败状态、提交号和日志，再决定是否修复或回退融合提交。
