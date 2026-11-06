# Codex++ Windows 云端构建复用说明

本文记录如何在 `jacywallny/CodexPlusPlus` 使用 GitHub Actions 构建 Windows 成品，适用于只验证云端编译、不改动本机安装的场景。

## 源码与分支

- 上游版本：`BigPizzaV3/CodexPlusPlus` 的 `v2.3.1`。
- 上游源码提交：`7d7832edfb93afbcc9759c8d970b07cbcb27ae44`。
- 云端验证分支：`ci/windows-v2.3.1-20261106`。
- 首次 CI 提交：`432beedc2fbaee18fb085596e46624184b2c5d84`。
- 本地检出：`D:\github\CodexPlusPlus-2.3.1-project-state-repair`。

同步时发现 fork 的 `main` 与上游已经分叉：上游领先 1527 个提交，fork 有 8 个独立提交。因此使用独立验证分支，没有强推或覆盖 `main`。该分支的 Rust、前端及注入脚本与上游 `v2.3.1` 相同，构建能力的调整位于 `.github/workflows/pr-build.yml`。旧分支上的定制功能尚未逐项移植到这个源码基线。

## 触发 Windows 构建

前提是 GitHub CLI 已登录，且对仓库有运行 Actions 的权限。下面的 `gh` 表示 PATH 中可用的 GitHub CLI；本机本次使用 `D:\DevTools\GitHubCLI\bin\gh.exe`。

```powershell
gh workflow run pr-build.yml --repo jacywallny/CodexPlusPlus --ref ci/windows-v2.3.1-20261106 -f platform=windows
```

`platform` 支持 `windows`、`macos`、`linux`、`all`，默认 `all`。需要通过 `--ref` 指定包含平台选择配置的分支；旧 `main` 尚没有这个新增参数。日常 Windows 验证使用 `windows`，避免同时启动其他平台的构建。

该分支当前仅手动触发。后续修改推送到此分支后，需再次运行上面的命令；仅推送此分支不会自动构建。

## 跟踪结果

触发命令返回本次 Actions 运行链接。获取和跟踪运行时，保留明确的运行编号与提交 SHA，避免误读其他版本的历史成功记录：

```powershell
gh run list --repo jacywallny/CodexPlusPlus --branch ci/windows-v2.3.1-20261106 --event workflow_dispatch --limit 5 --json databaseId,headSha,status,conclusion,url
gh run view <RUN_ID> --repo jacywallny/CodexPlusPlus
gh run watch <RUN_ID> --repo jacywallny/CodexPlusPlus --exit-status --interval 30
```

刚触发后的列表可能暂时为空；优先使用触发命令返回链接中的运行编号。只有运行最终 `conclusion=success` 且三个 Windows 产物都上传完成，才算本次交付成功。

构建顺序为前端依赖安装、前端测试、注入脚本回归检查、TypeScript 检查、Vite 构建、`cargo test --workspace --locked`、`cargo build --release --locked -p codex-plus-manager`、NSIS 打包与产物上传。

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

对照 `SHA256SUMS.txt`，并确认 `build-info.json` 的仓库、版本、提交、运行编号和测试筛选条件均符合预期。SHA-256 用于核对下载文件，不代表 Windows 代码签名或实机功能验证。分支构建的安装包文件名包含分支名，程序版本以构建信息中的 `version` 为准。

本流程只产出云端安装包和 EXE，不会自动发布 Release，也不会替换 `D:\Codex++`、重启 Codex、改变供应商或修复项目归属。

## 缓存与等待时间

- `actions/setup-node` 缓存 npm 下载内容，键关联 `package-lock.json`；仍执行 `npm ci`。
- `Swatinem/rust-cache` 缓存 Cargo 依赖和编译依赖产物，结合 Rust 工具链、依赖清单及构建环境区分缓存。
- 同一分支、同一平台的新运行取消尚未结束的旧运行。
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
gh workflow run release-assets.yml --repo jacywallny/CodexPlusPlus --ref ci/windows-v2.3.1-20261106 -f tag=v2.3.1 -f platform=windows -f publish=true -f windows_build_run_id=42233898399
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
