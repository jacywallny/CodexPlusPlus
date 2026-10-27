# HANDOVER — CodexPlusPlus

## 当前交接基线（2026-10-27）

- 当前融合工作树：`D:\github\CodexPlusPlus-rollout-compression-toggle-20261027`
- 当前已验证提交：`19190433764a4c2d7e10e43e14faa181c771e14d`
- 当前源码文档/部署脚本提交：`6e5f75f8`
- 当前版本：`2.0.0`
- 官方远端：`origin` → `BigPizzaV3/CodexPlusPlus`
- 个人远端：`fork` → `jacywallny/CodexPlusPlus`
- 个人 GitHub 分支：`codex/rollout-compression-toggle-20261027`
- Jacy 构建状态：`build-validation\2.0.0\status.json` 为 `complete`，核心、数据、launcher、前端和 release/probe 均通过。
- Jacy 持久构建目录：当前源码根下的 `target-jacy`。不要复用其他工作树的 target，也不要每次构建前清理。
- 详细操作入口：`docs/jacy-build-and-upstream-sync.md`。
- 本机 2.0.0 部署入口：`scripts/deploy-local-2.0.0.ps1`；先执行 `-PrepareOnly`，再在备份完成后正式部署。

本次已验证的是远程构建。安装到 `D:\Codex++`、备份现有安装、启动后回归属于单独的部署阶段，不能由构建状态代替。

## Active Work

### PR #1247: guard port auto-offset for multi-user RDP

**PR:** https://github.com/BigPizzaV3/CodexPlusPlus/pull/1247
**Branch:** `fix/guard-port-offset` (on lennney fork)

### What was done

Replaced hardcoded guard port constants with dynamic resolution:

| Before | After |
|--------|-------|
| `LAUNCHER_GUARD_PORT = 57320` | `launcher_guard_port()` — function with offset |
| `MANAGER_GUARD_PORT = 57319` | `manager_guard_port()` — function with offset |

Resolution order:
1. `CODEX_PLUS_GUARD_PORT` — exact port override
2. `CODEX_PLUS_{LAUNCHER,MANAGER}_GUARD_PORT` — per-role override
3. `CODEX_PLUS_GUARD_PORT_OFFSET` — explicit offset (e.g. +50)
4. Windows: `USERNAME` hash mod 1000 (auto per-user isolation)
5. Other platforms: 0 (backward compatible)

### CI Fix (2026-06-28 23:19)

**Problem:** All 3 platform builds failed with Rust E0308 type mismatch.
**Root cause:** `.and_then(|v| v.parse::<u16>().map_err(|_| ()))` — the `.or_else()` chain maintained `Result<_, VarError>` type but `.map_err(|_| ())` produced `Result<_, ()>`.
**Fix (5f9305f):** Switched to Option chain: `.ok().and_then(|v| v.parse().ok())`
**Code review:** APPROVED ✅
**CI:** Run 28326773669 in progress

### Files changed

| File | Change |
|------|--------|
| `crates/codex-plus-core/src/ports.rs` | +2 functions, +6 tests, base constants, Option chain fix |
| `apps/codex-plus-launcher/src/main.rs` | 4 references updated + test assertion |
| `apps/codex-plus-manager/src-tauri/src/lib.rs` | 5 references updated |
| `apps/codex-plus-manager/src-tauri/tests/windows_subsystem.rs` | 1 assertion updated |

### Commits

```
5f9305f fix(ports): use Option chain for env var parsing to resolve type mismatch
3a4f0aa fix(guard): auto-offset guard port by USERNAME for multi-user RDP
```

### Next steps

1. Wait for CI (run 28326773669) to complete — verify all 3 platforms green
2. Wait for upstream maintainer review
3. If maintainer requests changes, address them
