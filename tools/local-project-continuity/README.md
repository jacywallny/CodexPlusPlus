# 本地项目连续性修复

Codex++ 用户拓展，用于在同一 Windows 用户、同一 Codex home 内共享本地项目及有效会话归属，并在启动或身份变化后自动恢复。用户已明确选择跨订阅共享本机项目。

0.2.2 修复旧订阅缓存优先的覆盖风险：启动/身份变化时，已知本地项目的名称、根目录、排序、置顶、展示和会话归属采用最近正常保存的共享快照；冲突归属通过原生 `setMembership` 纠正，已保存的显式无项目选择也会恢复。正常运行继续采纳用户的新修改，未知本地项目及远程归属保留。19 项回归覆盖旧缓存冲突、移除源项目后临时成为无项目、再次修改后往返恢复。恢复仅处理元数据，不回滚科研工程文件。

0.2.1 补充保存已归属本地项目的 `thread-workspace-state-v1:<id>`：保留当前/待应用工作目录和工作区根目录。仅纳入现存线程、匹配本地项目的规定字段；0.2.2进一步规定以最新正常保存的归属/无项目选择决定恢复，远程归属不改动。2026-11-07 PMUT 76 条新目录已写入并核验，未主动进行真实订阅切换。

## 实测环境与证据

- Codex++ 2.3.1；Codex App 26.1103.3517.0；Windows。
- 2026-11-06：原生服务从 0 个项目恢复到 30 个项目、641 条现存会话归属；702 条历史引用中的 61 条无现存线程，未写回。
- 4 个置顶项目、30 项项目排序和 2 条按服务器项目 UUID 索引的侧栏自定义排序已读回；两条排序与初始备份一致，并已保存在共享快照。
- 已有引导完成记录恢复后，pending=false，角色 engineering、工作模式 coding。
- 10 项 Node 回归通过，包括身份变化、页面重新启动、主动删除、重新分类、同 home 映射合并、多窗口租约、过期 revision 拒绝和备份保留上限。
- **真实订阅切换后重启的一次方向已由用户确认侧栏可见且修复生效；切回原订阅的第二方向尚未记录。不得将模拟回归替代真实验收。**
- 2026-11-07：Codex App 26.1103.4239.0、拓展 0.2.0 已热加载；用户主动整理后为 12 个项目、297 条归属、18 个持久移除标记。原生服务与共享快照一致，1311 个会话 ID 和归档标志未变。
- 0.2.0 新增跨身份的 `removedProjects`：正常观察会记录项目移除；启动/身份变化恢复时先排除这些项目，旧身份仍含对应入口时通过原生 `projects.removeLocal` 移除入口。过滤对应排序、工作区根路径及服务器映射，不删除项目目录或会话。后续新增项目仍可保存。
- 12 项 Node 回归通过，补充旧身份仍保留项目时的重启/往返、移除元数据过滤及主动重新添加。对本机 6 份订阅配置与 8 份当前/历史缓存进行了 48 次恢复计划回放；这属于配置检查和模拟验证，未实际逐个切换订阅。
- 2026-11-07 23:41：0.2.2 已在原安装目录运行并读回 ready/error=null；最新 4 项目、327 条归属、27 个移除标记保持，76 条研究会话的新目录与共享快照匹配。19 项回归、空缓存/三份历史缓存/人工冲突缓存五组本机回放通过，原生元数据和工作区状态与操作前一致，安装四文件与源码 SHA-256 相同。本轮完成后台恢复后的验收，未真实切换订阅；计数仅代表采样时点。

## 安装与启动

在仓库根目录，用 PowerShell 7 执行：

```powershell
pwsh -File scripts/install-local-project-continuity.ps1 -DryRun
pwsh -File scripts/install-local-project-continuity.ps1
```

脚本先备份已有拓展和 user_scripts.json，再把四个运行文件复制到 `%APPDATA%\Codex++\user_scripts\local-project-continuity`。需要 Codex++ 用户拓展总开关已经开启；安装不会替用户启用其他拓展。安装后在 Codex++ 中重新加载拓展，或由用户正常重启 Codex++ 托管的 Codex。

菜单中的「本地项目连续性修复」可查看状态。诊断对象是 `window.__codexLocalProjectContinuity`，正常为 phase=ready、error=null。每 5 秒检查一次，启动和身份变化后恢复；正常运行只保存当前分类，因此主动删除和移动会话可以持久保留。

共享快照 v2 在顶层保存 `removedProjects`，防止已移除入口被旧身份缓存重新合并。不要为了恢复旧项目而覆盖整个快照。`capture-project-baseline.js` 只读导出项目元数据；`scripts/preserve-codex-project-list.mjs <已指定目录>` 生成 JSON/CSV/Markdown 和归属清单；`scripts/audit-project-list-subscriptions.mjs <快照目录> [原始快照目录]` 只读核对订阅设置、缓存恢复计划、会话 ID/归档标志及已安装源码哈希，不输出密钥。

快照与恢复前备份位于 `%APPDATA%\Codex++\extension-data\local-project-continuity\<Codex-home-hash>\`；安装备份位于其父级的 `installation-backups`。拓展不覆盖运行中的全局 JSON 或 SQLite；仅只读 SQLite 核实线程存在，项目修改通过原生运行服务完成。恢复前备份仅在确有恢复改动时生成，并自动按修改时间保留最新 3 份；清理只匹配本拓展生成的备份文件名，不触碰 `installation-backups` 或其他文件。

## 范围与兼容性

仅处理本机本地项目、有效会话归属、会话工作目录、相关侧栏元数据和已经完成的引导状态。不会迁移凭据、权限选择/批准记录、提示历史或 ChatGPT 云项目 backing。会话工作区根目录属于路径选择，随其一起保存。不同 Codex home 使用不同快照。

拓展文件独立于 Codex++ EXE，正常更新 EXE 不会替换该目录。但未来 Codex App 若调整状态结构、原生接口或模块导出，可能需要适配；不支持的结构会停止本轮操作并显示错误。已有备份保持不变。

## 核验

```powershell
node --experimental-vm-modules --test tools/local-project-continuity/authority.test.mjs tools/local-project-continuity/continuity.test.mjs tools/local-project-continuity/renderer.test.mjs
node scripts/audit-local-project-continuity.mjs 'C:\Users\18127\.codex'
node scripts/local-project-state-cdp.mjs tools/local-project-continuity/check-runtime.js 9229
```

最后一个命令仅用于本机已开启的调试端口，不依赖历史 probe。端口由当前 Codex++ 实际配置确定，可查看 `%USERPROFILE%\.codex-session-delete\latest-status.json`；不要对外开放。该命令只读，返回原生计数、稳定内容哈希、页面启动时间、身份指纹及引导状态，不返回凭据。

真实验收：保存切换前结果 → 用户手动切换订阅 → 确认身份或页面启动时间发生变化 → 对比原生项目和归属计数与内容哈希、拓展 ready 状态及引导状态 → 用户确认侧栏可见且没有首次使用引导 → 再手动切回并复核。若期间用户主动调整分类，应以最新状态为基线。

## 禁用与回退

在 Codex++ 用户拓展中禁用「本地项目连续性修复」，再重新加载拓展。禁用只停止后续自动同步，当前已恢复的项目和历史仍保留。不需要删除 auth.json、config.toml、sessions、state_*.sqlite 或全局状态文件。

若只需回退拓展版本，可从安装备份还原四个运行文件，再重新加载；不要用旧 user_scripts.json 覆盖安装后对其他拓展所做的调整。若要回退项目元数据，先选定备份并通过原生服务按字段恢复，禁止把旧完整状态覆盖进在线文件。
