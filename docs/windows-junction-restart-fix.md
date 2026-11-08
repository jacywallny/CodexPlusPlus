# Windows 应用卷目录联接重启修复

此修复基于上游 `v2.4.0`（`8826008d5eb921ed99dae88c4df803c328041b81`），按用户要求发布独立版本 `2.4.1`。

Windows 将 MSIX 应用移至其他应用卷时，安装登记与命令行可能保留 C 盘的 WindowsApps 入口，而原生进程查询返回 D 盘真实映像。两个路径经目录联接指向同一入口，原停止逻辑却在安装路径比较或 argv[0] 比较中拒绝。

`watcher/windows_main_stop.rs` 新增可执行文件比较：先保留原路径相等行为；不同路径时仅在 Windows 两端 `canonicalize` 都成功且结果一致时认定为同一个入口。原有包更新匹配仍保留，但不能用于放宽 argv[0] 的入口匹配。不能解析的跨根路径不会按包名猜测相同。

用户 SID、登录身份、Windows 会话、APPDATA、用户数据目录、agent 隔离、子进程排除、出生身份和持有句柄的重复核验不变。用户数据目录仍采用原有路径约定。

测试新增真实目录联接的双向安装/映像/argv 组合，拒绝不同文件和不可解析入口，验证隔离边界；原公开停止 API 测试增加通过目录联接启动真实测试子进程的模式，仅允许停止所选实例。

## 云端构建和发布

流程复用历史云端构建方案，保留本版本前端、注入脚本和 workspace 测试。以下分支需已推送至个人 fork：

```powershell
gh workflow run pr-build.yml --repo jacywallny/CodexPlusPlus --ref fix/windows-junction-restart-20261108 -f platform=windows
```

必须确认该提交的完整测试、Release 构建及三个 Windows Artifacts 成功，核对 `build-info.json`、SHA-256 和运行编号后，再将轻量标签 `v2.4.1` 预建为同一个提交。标签不可覆盖。接着按实际成功编号运行：

```powershell
gh workflow run release-assets.yml --repo jacywallny/CodexPlusPlus --ref fix/windows-junction-restart-20261108 -f tag=v2.4.1 -f platform=windows -f publish=true -f windows_build_run_id=<完整测试成功运行编号>
```

工作流核对成功运行、来源仓库、提交、版本、无测试筛选及 EXE 哈希，重新打包并发布 Windows 安装包和便携 ZIP。发布属于个人 fork，区别于上游 2.4.0。

CI 测试和成品发布不等于本机实际重启验收。部署时仍须保留当前 Codex++ 配置与用户脚本，核验真实停止/重启后再记录实测结果。此任务不迁移官方 Codex，不修改系统目录联接或 ACL。
