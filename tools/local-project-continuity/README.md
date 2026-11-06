# 本地项目连续性修复

Codex++ 用户拓展，用于在同一 Windows 用户、同一 Codex home 内共享本地项目及有效会话归属，并在启动或身份变化后自动恢复。用户已明确选择跨订阅共享本机项目。

## 实测环境与证据

- Codex++ 2.3.1；Codex App 26.1103.3517.0；Windows。
- 2026-11-06：原生服务从 0 个项目恢复到 30 个项目、641 条现存会话归属；702 条历史引用中的 61 条无现存线程，未写回。
- 4 个置顶项目、30 项项目排序和 2 条按服务器项目 UUID 索引的侧栏自定义排序已读回；两条排序与初始备份一致，并已保存在共享快照。
- 已有引导完成记录恢复后，pending=false，角色 engineering、工作模式 coding。
- 10 项 Node 回归通过，包括身份变化、页面重新启动、主动删除、重新分类、同 home 映射合并、多窗口租约、过期 revision 拒绝和备份保留上限。
- **真实订阅切换后重启的一次方向已由用户确认侧栏可见且修复生效；切回原订阅的第二方向尚未记录。不得将模拟回归替代真实验收。**

## 安装与启动

在仓库根目录，用 PowerShell 7 执行：

```powershell
pwsh -File scripts/install-local-project-continuity.ps1 -DryRun
pwsh -File scripts/install-local-project-continuity.ps1
```

脚本先备份已有拓展和 user_scripts.json，再把四个运行文件复制到 `%APPDATA%\Codex++\user_scripts\local-project-continuity`。需要 Codex++ 用户拓展总开关已经开启；安装不会替用户启用其他拓展。安装后在 Codex++ 中重新加载拓展，或由用户正常重启 Codex++ 托管的 Codex。

菜单中的「本地项目连续性修复」可查看状态。诊断对象是 `window.__codexLocalProjectContinuity`，正常为 phase=ready、error=null。每 5 秒检查一次，启动和身份变化后恢复；正常运行只保存当前分类，因此主动删除和移动会话可以持久保留。

快照与恢复前备份位于 `%APPDATA%\Codex++\extension-data\local-project-continuity\<Codex-home-hash>\`；安装备份位于其父级的 `installation-backups`。拓展不覆盖运行中的全局 JSON 或 SQLite；仅只读 SQLite 核实线程存在，项目修改通过原生运行服务完成。恢复前备份仅在确有恢复改动时生成，并自动按修改时间保留最新 3 份；清理只匹配本拓展生成的备份文件名，不触碰 `installation-backups` 或其他文件。

## 范围与兼容性

仅处理本机本地项目、有效会话归属、相关侧栏元数据和已经完成的引导状态。不会迁移凭据、权限、提示历史或 ChatGPT 云项目 backing。不同 Codex home 使用不同快照。

拓展文件独立于 Codex++ EXE，正常更新 EXE 不会替换该目录。但未来 Codex App 若调整状态结构、原生接口或模块导出，可能需要适配；不支持的结构会停止本轮操作并显示错误。已有备份保持不变。

## 核验

```powershell
node --experimental-vm-modules --test tools/local-project-continuity/continuity.test.mjs tools/local-project-continuity/renderer.test.mjs
node scripts/audit-local-project-continuity.mjs 'C:\Users\18127\.codex'
node scripts/local-project-state-cdp.mjs tools/local-project-continuity/check-runtime.js 9229
```

最后一个命令仅用于本机已开启的调试端口，不依赖历史 probe。端口由当前 Codex++ 实际配置确定，可查看 `%USERPROFILE%\.codex-session-delete\latest-status.json`；不要对外开放。该命令只读，返回原生计数、稳定内容哈希、页面启动时间、身份指纹及引导状态，不返回凭据。

真实验收：保存切换前结果 → 用户手动切换订阅 → 确认身份或页面启动时间发生变化 → 对比原生项目和归属计数与内容哈希、拓展 ready 状态及引导状态 → 用户确认侧栏可见且没有首次使用引导 → 再手动切回并复核。若期间用户主动调整分类，应以最新状态为基线。

## 禁用与回退

在 Codex++ 用户拓展中禁用「本地项目连续性修复」，再重新加载拓展。禁用只停止后续自动同步，当前已恢复的项目和历史仍保留。不需要删除 auth.json、config.toml、sessions、state_*.sqlite 或全局状态文件。

若只需回退拓展版本，可从安装备份还原四个运行文件，再重新加载；不要用旧 user_scripts.json 覆盖安装后对其他拓展所做的调整。若要回退项目元数据，先选定备份并通过原生服务按字段恢复，禁止把旧完整状态覆盖进在线文件。
