# Codex++ Windows 云端构建复用说明

本文记录如何在 `jacywallny/CodexPlusPlus` 使用 GitHub Actions 构建 Windows 成品，适用于只验证云端编译、不改动本机安装的场景。

## 2.8.3 串行与并行构建对照（2026-11-21）

在同一个个人 fork 提交上，手动触发两个模式；不设置 Rust 测试筛选。验证和打包使用相同定义、Windows runner 类型和缓存设置。串行模式等待完整验证成功后才调度打包 runner；并行模式同时调度。两者都在验证和打包成功后交付正式命名的产物。

```powershell
gh workflow run pr-build.yml --repo jacywallny/CodexPlusPlus --ref fix/personal-windows-tray-v2.8.3 -f platform=windows-serial
gh workflow run pr-build.yml --repo jacywallny/CodexPlusPlus --ref fix/personal-windows-tray-v2.8.3 -f platform=windows
```

对照时核对两轮的 head SHA、工具链和实际缓存命中；分别记录排队、验证、Release 编译、NSIS 打包和产物可用时间。并行主要缩短等待时间，不能用总耗时推导编译器提速。若一轮缓存未命中或排队异常，应单列条件，必要时重跑对照。

## Windows CI 提速初稿（2026-11-20）

基线为个人 fork `main` 的 `8863002d`。v2.8.2 的 [冷缓存运行 #44701908799](https://github.com/jacywallny/CodexPlusPlus/actions/runs/44701908799) 用时 32 分 05 秒：Rust 测试 12 分 17 秒，data 串行测试 5 分 08 秒，Release 编译 11 分 17 秒，NSIS 打包 57 秒。随后 `main` 已预热测试和 Release 两份 Windows 缓存。这个基线用于比较，不能代表缓存命中后的耗时。

优化放在个人 fork 分支 `ci/windows-build-speedup`。当前仅静态检查，不触发编译、测试、打包或发布。启用后，`platform=windows` 让两个 Windows runner 分别跑验证和构建；两者通过后，Ubuntu runner 才上传三个原名产物。`windows-preview` 只出独立的自测包，保留 7 天；正式发布会在创建标签前拒绝缺少正式产物的运行，并保留版本、提交、完整测试及摘要校验。

等待需要构建时再手动触发；下面两条命令任选其一，目前都不执行：

```powershell
gh workflow run pr-build.yml --repo jacywallny/CodexPlusPlus --ref ci/windows-build-speedup -f platform=windows
gh workflow run pr-build.yml --repo jacywallny/CodexPlusPlus --ref ci/windows-build-speedup -f platform=windows-preview
```

首次启用先跑 `windows`，核对两个 Windows job 并行、`windows-release` 缓存恢复结果、三份原名产物与 SHA-256；再用同一提交比较命中情况和总耗时。日常构建和验证都在 GitHub Actions 执行。正式复用路径仍是 `release-assets.yml` 的 `windows_build_run_id`。

## 2.7.2 修复构建（2026-11-17）

个人 fork 基于上游 `v2.7.1`（`d5dcad3b3b9d1ef1dc83ba76a3b8f4c4098d2abe`）修复 #3151、#3154、#3155，版本升为 `2.7.2`。保留个人仓库原有历史与云端构建能力，不改写历史。

- 修复分支：`fix/ws-context-and-http-fallback`。
- 修复实现：`51096e7f335ad81e7a207d619e48163b214b9e2b`。
- 本轮测试和打包提交：`971bf162b1d45a2a6d1153ec26c5fd62c921311d`。
- Windows 全量运行：`https://github.com/jacywallny/CodexPlusPlus/actions/runs/44075197454`，未设置 Rust 测试筛选。
- 验证结果：2026-11-17 19:34:12 至 20:04:00（Asia/Shanghai）运行成功，总耗时 29 分 48 秒。前端测试、注入回归、TypeScript 检查、Vite 构建和全部 Windows 打包步骤通过。
- Rust 汇总：5420 通过、0 失败、47 忽略。统计 21 组 `0 filtered out` 的测试结果，不重复计入独立子进程的定向摘要；忽略项没有执行。
- 新增的五项 Rust 回归全部通过；版本脚本 LF/CRLF 两项 Node 测试通过；六处版本元数据一致为 `2.7.2`。
- Rust workspace 编译与测试 11 分 20 秒，data 串行编译与测试 5 分 25 秒；Release 编译 10 分 03 秒，NSIS 打包与版本检查 45 秒。云端工具链为 `rustc 1.100.0 (0ef1bc386 2026-11-09)`。
- 成品：`codex-plus-plus-windows-binaries`（Artifact 14326257724）、`codex-plus-plus-windows-installer`（14325873594）、`codex-plus-plus-windows-build-info`（14325074983），均已上传且未过期。安装包文件名为 `CodexPlusPlus-2.7.2-windows-x64-setup.exe`。
- 构建验证后的记录提交只更新文档；发布阶段仅调整 Release 说明的通用前缀，程序代码及测试、打包逻辑保持一致。未在本机下载、安装或执行本次成品。

官方账号网关的 WebSocket 请求保留响应、输入项和工具调用标识；纯工具增量沿用首轮缓存键，未知续接 ID 明确拒绝。第三方 Responses 供应商默认回退 HTTP；确实支持原生 Responses WebSocket 时，在供应商配置快照的活动 provider 下显式声明：

```toml
model_provider = "custom"

[model_providers.custom]
supports_websockets = true
```

该声明不替代原有 provider 的 URL、认证和其他配置，需与真实上游能力一致。显式 `false` 禁用；路由池中存在不支持的成员时，现有保守检查让整个入口在升级前返回 426。没有增加 WS 到 HTTP 的桥接或生成后自动重放。

新增测试使用本地模拟上游覆盖官方账号网关连续两轮工具调用、缓存键及连接复用、未知续接拒绝和握手前回退。云端通过只能证明这些测试及构建通过，真实账号和客户端仍需实机验证。

### 2.7.2 正式发布

2026-11-17 21:39:51（Asia/Shanghai）已在个人仓库公开发布，非草稿、非预发布，`releases/latest` 已确认为 `v2.7.2`。

- Release：`https://github.com/jacywallny/CodexPlusPlus/releases/tag/v2.7.2`。
- 发布运行：`https://github.com/jacywallny/CodexPlusPlus/actions/runs/44094615472`，最终成功，21:37:38 至 21:39:54，耗时 2 分 16 秒。
- 版本标签精确指向完整测试通过的 `971bf162b1d45a2a6d1153ec26c5fd62c921311d`；发布工作流提交为 `52575d8ceed95b221dd7f23ccae64c82a92e602e`。
- Windows EXE 复用构建运行 `44075197454`，来源、版本、完整测试标记和 SHA-256 均由发布流程校验；NSIS 安装包和便携 ZIP 重新打包。
- 五个非空附件均已上传：`CodexPlusPlus-2.7.2-windows-x64-setup.exe`、`CodexPlusPlus-2.7.2-windows-x64.zip`、`SHA256SUMS.txt`、`build-info.json`、`latest.json`。本次只发布 Windows x64。
- 已从 Release 读取三份元数据并核对：构建版本 `2.7.2`、源码提交、构建及发布运行编号、`testFilter=""`，更新元数据版本 `v2.7.2` 和正式 tag 地址均一致。安装包和 ZIP 的 GitHub asset digest 与 `SHA256SUMS.txt` 一致。

```text
92b94368dccef099ca4533cdd964ec5855ad45a15b701127a01ee85bde17ba0f  codex-plus-plus.exe
219a93e2f0011eadbeac0793863c2ad7df69936df85d3450a4cc22fe483993d1  CodexPlusPlus-2.7.2-windows-x64-setup.exe
347ad9cd33ecd1dc6bcd6f3049791e64086a995092036defe3371686d1536964  CodexPlusPlus-2.7.2-windows-x64.zip
```

EXE 摘要来自已核验的 `build-info.json`，安装包和 ZIP 摘要来自 Release 清单与 GitHub asset digest；本机没有下载和重新计算成品摘要。实际账号行为仍需安装后实测。

## 2.3.1 历史基线（2026-11-06）

- 上游版本：`BigPizzaV3/CodexPlusPlus` 的 `v2.3.1`。
- 上游源码提交：`7d7832edfb93afbcc9759c8d970b07cbcb27ae44`。
- 当时维护分支：`main`，源码基线为已验证的 `2.3.1`。
- 首次云端验证分支：`ci/windows-v2.3.1-20261106`，已在分支收敛时归档。
- 首次 CI 提交：`432beedc2fbaee18fb085596e46624184b2c5d84`。
- 当时本地检出：`D:\github\CodexPlusPlus-2.3.1-project-state-repair`。

首次同步时 fork 的旧 `main` 与上游已经分叉：上游领先 1527 个提交，fork 有 8 个独立提交，当时使用独立验证分支。发布成功后，按用户确认，将已验证的 `2.3.1` 和 CI 优化整理为唯一维护分支 `main`；通过保留旧 `main` 为合并父提交的方式正常推进，没有强推改写历史。当时 Rust、前端及注入脚本与已验证构建相同；旧分支上的定制功能保存在归档中，尚未逐项移植到这个源码基线。

## 触发 Windows 构建

前提是 GitHub CLI 已登录，且对仓库有运行 Actions 的权限。下面的 `gh` 表示 PATH 中可用的 GitHub CLI；本机本次使用 `D:\DevTools\GitHubCLI\bin\gh.exe`。

```powershell
gh workflow run pr-build.yml --repo jacywallny/CodexPlusPlus --ref main -f platform=windows
```

`platform` 支持 `windows`、`macos`、`linux`、`all`，默认 `all`。配置已经并入 `main`。日常手动 Windows 验证使用 `--ref main -f platform=windows`，避免同时启动其他平台的构建。

`main` 的普通推送会按现有配置自动触发各平台检查；需要仅验证 Windows 时使用上述手动命令。2.7.2 使用已经通过的精确提交完成云端验证后推进 `main`，后续记录提交只更新文档，带 `[skip ci]`，不重复触发完整构建。

## 跟踪结果

触发命令返回本次 Actions 运行链接。获取和跟踪运行时，保留明确的运行编号与提交 SHA，避免误读其他版本的历史成功记录：

```powershell
gh run list --repo jacywallny/CodexPlusPlus --branch main --event workflow_dispatch --limit 5 --json databaseId,headSha,status,conclusion,url
gh run view <RUN_ID> --repo jacywallny/CodexPlusPlus
gh run watch <RUN_ID> --repo jacywallny/CodexPlusPlus --exit-status --interval 30
```

刚触发后的列表可能暂时为空；优先使用触发命令返回链接中的运行编号。只有运行最终 `conclusion=success` 且三个 Windows 产物都上传完成，才算本次交付成功。

在提速分支上，Windows 验证 job 运行前端测试、注入脚本回归、TypeScript 检查和 Vite，再运行 `cargo test --workspace --locked --exclude codex-plus-data`，最后单独串行运行 `cargo test --locked -p codex-plus-data -- --test-threads=1`，以遵守共享内存预算。手动打包 job 同时运行 Vite、`cargo build --release --locked -p codex-plus-manager` 和 NSIS；两路结束后才上传正式交付产物。尚未合入提速改动的分支继续使用原串行流程。

不要为了得到绿色状态而跳过失败测试。可用下面的命令查看失败证据；修正 CI 问题后，提交、推送并针对新提交重新触发：

```powershell
gh run view <RUN_ID> --repo jacywallny/CodexPlusPlus --log-failed
```

`rust_test_filter` 仅用于定向排障；填写它会缩小 Rust 测试范围，不能把这类运行记为全量测试通过。

## 成品与校验

成功运行会上传三个 Artifacts：

| 名称 | 内容 |
| --- | --- |
| `codex-plus-plus-windows-binaries` | `codex-plus-plus.exe` |
| `codex-plus-plus-windows-installer` | Windows x64 NSIS 安装包 |
| `codex-plus-plus-windows-build-info` | `build-info.json` 和 `SHA256SUMS.txt` |

先自行确定下载目录，再下载指定运行的产物。下面的路径是占位符，不能原样执行：

```powershell
gh run download <RUN_ID> --repo jacywallny/CodexPlusPlus -n codex-plus-plus-windows-binaries --dir '<选定目录>\app'
gh run download <RUN_ID> --repo jacywallny/CodexPlusPlus -n codex-plus-plus-windows-installer --dir '<选定目录>\installer'
gh run download <RUN_ID> --repo jacywallny/CodexPlusPlus -n codex-plus-plus-windows-build-info --dir '<选定目录>\metadata'
Get-FileHash -LiteralPath '<选定目录>\app\codex-plus-plus.exe' -Algorithm SHA256
```

对照 `SHA256SUMS.txt`，并确认 `build-info.json` 的仓库、版本、提交、运行编号和测试筛选条件均符合预期。SHA-256 用于核对下载文件，不代表 Windows 代码签名或实机功能验证。从 2.7.2 起，安装包文件名和安装登记版本使用程序版本，不再使用分支名；来源分支和提交记录在 `build-info.json`。

本流程只产出云端安装包和 EXE，不会自动发布 Release，也不会替换 `D:\Codex++`、重启 Codex、改变供应商或修复项目归属。

## 缓存与等待时间

- `actions/setup-node` 缓存 npm 下载内容，键关联 `package-lock.json`；仍执行 `npm ci`。
- `Swatinem/rust-cache` 缓存 Cargo 依赖和编译依赖产物，结合 Rust 工具链、依赖清单及构建环境区分缓存。
- Windows 测试 job 保留 `windows` 缓存；打包 job 读取 `release-cache.yml` 在 `main` 预热的 `windows-release`，两个 profile 不混用。预热编译目标与手动打包一致，并复用 npm 缓存；复用现成 EXE 发版时跳过 Rust 缓存下载。
- 同一分支的自动推送构建会取消旧的自动运行；手动构建按运行编号独立，不自动互相取消。
- Windows 发布构建只编译管理器包，Rust 测试仍覆盖整个 workspace。

首次运行没有该分支的缓存，仍可能花费几十分钟。只有实际比较后续缓存命中运行，才能判断加速幅度；更换工具链、依赖或分支可能导致缓存未命中。GitHub 排队与缓存上传也计入等待时间。

## fork 的 Actions 被平台暂停时

本次遇到的错误为 `HTTP 422: Actions has been disabled for this repository.`。同时，权限 API 仍返回 `enabled=true`，工作流状态也是 `active`，因此这两个接口不足以证明 fork 能运行。

应使用已登录的仓库维护者账号打开仓库的 Actions 页面。若显示 `Workflows on this fork have been disabled`，检查工作流用途后点击 `I understand my workflows, go ahead and re-enable them`。本次网页操作后显示“操作已启用”，随后命令行触发成功。

只调用 `gh workflow enable pr-build.yml` 或更新仓库 Actions 权限，没有解除本次 fork 层面的暂停。遇到同样的提示时，不必重复改 YAML 或重编译本机应用。

## 本次验证记录

2026-11-06（Asia/Shanghai）实际运行成功，GitHub 运行编号 `42233898399`，第 1 次尝试；构建提交为 `432beedc2fbaee18fb085596e46624184b2c5d84`。运行从 22:26:14 开始，总耗时 **22 分 32 秒**；Windows 作业成功，macOS 与 Linux 按输入参数跳过。

运行记录：`https://github.com/jacywallny/CodexPlusPlus/actions/runs/42233898399`。

| 阶段 | 结果 | 耗时 |
| --- | --- | --- |
| 前端测试、注入回归、TypeScript 检查、Vite 构建 | 成功 | 各步骤已核对 |
| Rust workspace 测试（含首次测试编译） | 4722 通过、0 失败、44 忽略；未设置测试筛选 | 11 分 04 秒 |
| Release 管理器编译 | 成功 | 8 分 16 秒 |
| NSIS 安装包 | 成功 | 33 秒 |
| EXE、安装包、构建信息上传 | 三个 Artifacts 均成功 | 已核对 |

构建信息确认程序版本 `2.3.1`、仓库 `jacywallny/CodexPlusPlus`、Windows runner、测试筛选为空，提交与运行编号正确。实际 Rust 工具链为 `rustc 1.99.1 (58cf8f9c3 2026-10-13)`。

4722/0/44 是该次 `cargo test` 日志中 18 组结果的通过、失败、忽略总和；44 项忽略用例没有执行，不能记为通过。缓存 API 已确认该分支保存了 npm 缓存 40,474,693 字节和 Rust 缓存 1,182,898,377 字节。

CI 生成的文件 SHA-256：

```text
33446f5ec26fdcee7869757e783cfe4dcd9f11903d17a865bcaeec31de9ad0bf  codex-plus-plus.exe
afce36b1026a980a8cce3d6ca7a5ec747df321288659e2c17ce3210c778aeeb9  CodexPlusPlus-ci-windows-v2.3.1-20261106-windows-x64-setup.exe
```

GitHub 在 Artifacts 列表显示的 digest 是产物压缩包的摘要，与这里的 EXE 文件摘要对象不同，不能混用。

已将 EXE 与安装包下载到任务临时工作目录，并分别计算 SHA-256；两者均与上述 CI 清单一致。下载副本没有执行。

本次证明该提交可在 GitHub Windows runner 上通过流程、编译和打包。没有运行或安装产物，没有验证供应商切换后的项目恢复行为，也没有执行第二轮缓存命中耗时对比。后续补充本文档的提交不改变本次已经核验的构建 SHA。

## 发布已通过的 Windows 构建

本 fork 的发布流程支持仅发布 Windows，并直接复用同仓库成功的 `PR build artifacts` 运行，免于再次编译 Rust 和前端。完整测试运行编号、版本、提交及 EXE 的 SHA-256 必须全部匹配；使用过 Rust 测试筛选的产物会被拒绝。正式 NSIS 安装包会用已校验的 EXE 重新打包，确保安装登记版本使用正式版本号。

```powershell
gh workflow run release-assets.yml --repo jacywallny/CodexPlusPlus --ref main -f tag=v2.3.1 -f platform=windows -f publish=true -f windows_build_run_id=42233898399
```

该命令会公开发布版本，只有确认需要发版时才执行。日常仅验证构建时使用前面的 `pr-build.yml` 命令。

`windows_build_run_id` 留空时仍采用原有重新编译流程；默认 `platform=all` 的双平台完整性检查保持不变。显式选择 `windows` 时只要求 Windows 安装包、便携 ZIP 和校验清单齐备，再生成 `latest.json` 并自动发布。整个过程由 `Release assets` 工作流管理草稿和发布，不手工创建 Release。

复用模式首次发版会用工作流的 `GITHUB_TOKEN` 创建缺失的 fork 标签，使标签精确指向成功构建的提交；此操作不会触发另一轮 tag push 构建。已有标签与构建不一致时会拒绝操作，禁止覆盖标签。

如果目标提交含相对默认分支的工作流修改，GitHub 可能在创建标签时返回 `403 Resource not accessible by integration`；本次首轮运行 `42241055387` 即遇到这一限制。无需向仓库添加个人令牌密钥，可以用已登录维护者的 CLI 预建精确标签，再运行上面的命令。为避免预建标签另起默认双平台发版，暂挂的仅是 `Release assets`，并在 `finally` 中恢复：

```powershell
$releaseWorkflowDisabled = $false
try {
    gh workflow disable release-assets.yml --repo jacywallny/CodexPlusPlus
    if ($LASTEXITCODE -ne 0) { throw '暂挂自动 tag 发布失败' }
    $releaseWorkflowDisabled = $true
    gh api --method POST repos/jacywallny/CodexPlusPlus/git/refs -f ref=refs/tags/v2.3.1 -f sha=432beedc2fbaee18fb085596e46624184b2c5d84
    if ($LASTEXITCODE -ne 0) { throw '创建发布标签失败' }
} finally {
    if ($releaseWorkflowDisabled) {
        gh workflow enable release-assets.yml --repo jacywallny/CodexPlusPlus
        if ($LASTEXITCODE -ne 0) { throw '重新启用发布工作流失败' }
    }
}
```

标签已存在时跳过预建，不删除、移动或覆盖它。后续版本替换命令前，应先核对实际版本、成功运行编号与提交，不能照抄本次提交。

本 fork 的 `v2.3.1` 标签对应已验证提交 `432beedc2fbaee18fb085596e46624184b2c5d84`，其中应用源码与上游 `v2.3.1` 相同，另含 CI 调整。上游同名标签仍指向 `7d7832edfb93afbcc9759c8d970b07cbcb27ae44`；从两个远端获取标签时注意区分来源，不强制覆盖本地标签。

发布说明会注明仅提供 Windows，以及旧定制分支未逐项合入、此次未新增项目恢复修复等边界。该流程不会部署本机。

## 本次正式发布结果

2026-11-06 **23:17:36（Asia/Shanghai）**，`jacywallny/CodexPlusPlus` 的 `v2.3.1` 已公开发布，`draft=false`、`prerelease=false`，并标为 Latest。

- Release：`https://github.com/jacywallny/CodexPlusPlus/releases/tag/v2.3.1`。
- 成功发版运行：`https://github.com/jacywallny/CodexPlusPlus/actions/runs/42241273288`。
- 工作流提交：`15dba0236f85095e776a3be53d1818a0d2dc6d5f`，运行 23:15:03–23:17:39，总耗时 **2分36秒**。
- Windows 作业成功，macOS 按平台参数跳过；正式 NSIS 打包44秒。
- 发布附件共5个：Windows 安装包、便携 ZIP、`SHA256SUMS.txt`、`build-info.json`、`latest.json`。

本次复用了运行 `42233898399` 的 EXE，没有重新编译，也没有重跑该次已通过的完整测试；2分36秒是这次复用与重新打包发布的耗时，不能当作一次完整构建的耗时。

正式发布文件 SHA-256：

```text
4acaa81fe1a04517d5ffc8360ab180eb1b5e6cfad075e960ec4ddc8a04db5c9e  CodexPlusPlus-2.3.1-windows-x64-setup.exe
a579f539bee7940bc18356069f9a4492d5dddb43481a4547777ba18a29c3e4f9  CodexPlusPlus-2.3.1-windows-x64.zip
```

已下载公开 Release 的5个附件：安装包与 ZIP 的本地 SHA-256 均与清单一致，ZIP 只包含一个 Windows x64 的 `codex-plus-plus.exe`，其 SHA-256 与首次 CI 的 `33446f5ec26fdcee7869757e783cfe4dcd9f11903d17a865bcaeec31de9ad0bf` 一致。`build-info.json` 的源提交、版本和两个运行编号均匹配；`latest.json` 的版本、说明、正式页面地址及附件列表一致。

这些副本仅用于核验，没有执行或安装。本地证据为任务工作目录 `work/release-2.3.1/verification-summary.json` 与 `verify-release.py`。

## 分支收敛与恢复

2026-11-06 以已发布的 `2.3.1` 和云端构建优化作为 `main`，其他14个远程分支清理前，已为全部15个原分支建立并验证恢复标签：

```text
archive/branches/20261106-233558/<原分支名>
```

其中旧 `main` 为 `ea39facf149487fcd44553595cf1a0b28372c98c`，归档标签为 `archive/branches/20261106-233558/main`；验证分支归档保留整理前的提交 `4545c9786e4382e7c344ecad706db9a4709dd5f4`。这些是恢复标签，不作为新的发布版本，也不触发 `v*` 发版工作流。

要检查旧代码，可从标签恢复到单独的本地分支，再决定后续处理：

```powershell
git fetch fork tag archive/branches/20261106-233558/main
git switch -c recovered-old-main archive/branches/20261106-233558/main
```

旧版 PR #1 的来源 `codex/provider-startup-repair` 同样已归档；PR 记录保留，按此次只保留新 `main` 的安排关闭。GitHub Release、版本标签、已发布附件和缓存均不主动删除；其他本地检出目录也不清理。本次没有重新编译，因此迁移到 `main` 后首次构建的缓存命中与耗时仍待实测。
