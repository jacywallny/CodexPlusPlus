# Windows 供应商切换启动修复

## 问题与证据

2026-10-03 的本机日志连续七次报告 `Provider sync skipped: failed to replace ...`。
启动器要求供应商同步成功后才启动 Codex，因此一次 rollout 替换失败会中止启动。
这些启动尝试约耗时 40–56 秒；另有一次重启等待同步结束达 30 秒。

Windows `MoveFileExW` 的 `windows::core::Error` 经 anyhow 传递后，不是同步层检查的
`std::io::Error`。同步层的字符串后备检查仅包含共享/锁冲突编码，未覆盖访问拒绝编码。
可读取但没有 `FILE_SHARE_DELETE` 的文件能复现同样的替换失败和整体同步中止。
原子写入和供应商同步的两项 Windows 回归在修复前都失败。

此前的流式历史修复、侧栏保护、2026-09-21 的 Windows 替换重试/进程等待，以及
可选 Windows 运行时启动检查均保留。本次没有关闭自动历史修复、删除历史或直接修改本机数据库。

## 修复行为

- 将 `HRESULT_FROM_WIN32` 的原始错误码保留为 `std::io::Error`。现有同步逻辑因此能
  跳过被占用的 rollout，继续其他历史和索引的同步；文件解除占用后下次同步会重试。
  原内容不被强制覆盖；其他错误继续沿用失败和回滚机制。
- 自动启动使用独立的增量扫描入口。手动“修复历史会话”仍完整扫描原文件。
- 缓存只记录文件指纹、扫描事实、摘要和元数据位置，不保存会话正文、指令或
  `session_meta` 文本。命中前检查大小、纳秒修改时间、创建时间、Windows NTFS
  `ChangeTime`（Unix 使用 ctime），并核验元数据行摘要和缓存事实校验和。
- 文件变化、缓存缺失/损坏/不可用时回退完整扫描。删除和归档移动会刷新路径集合。
  多条元数据、CRLF、供应商长度变化和当前子代理分类均保留。
- 真正重写文件前仍核验完整 SHA-256；缓存不能跳过写入前的并发变更保护。
  重写后更新缓存指纹和元数据位置，避免下一次启动再次全量读取刚同步的文件。
  刷新前对重写结果进行完整 SHA-256 核验，并在读取前后检查指纹；即使只改正文、
  保持元数据行不变，也不能把并发修改后的文件与旧扫描事实绑定。
- 普通事件行通过忽略 payload 的轻量反序列化筛选，不为每条消息和工具结果构造
  完整 JSON Value。元数据、转义的类型值和重复 type 字段保留此前解析语义。

## 验证与构建

沿用已有 Windows MSVC/Rust 环境，不安装新工具链或依赖。

```powershell
$env:CODEX_GATE_TEST_POWERSHELL = (Get-Command pwsh.exe).Source
cargo test -p codex-plus-core --locked --offline
cargo test -p codex-plus-data --locked --offline
cargo test -p codex-plus-launcher --locked --offline
cargo test -p codex-plus-data --lib startup_cache_benchmark --locked --offline -- --ignored --nocapture
cargo build -p codex-plus-launcher -p codex-plus-manager --release --locked --offline
```

新增回归覆盖可读但禁止替换的 Windows 文件、被占用文件解除后的重试、缓存复用、
缓存文本隐私、追加/归档/删除、保留时间戳的同长度内容变化、缓存损坏、连续供应商
切换、多条元数据、写入前并发变化和子代理分类。既有同步/索引/回滚测试继续运行。

诊断事件 `provider_sync.scan_completed` 仅报告文件数、缓存命中数、完整扫描字节数
和时间；`provider_sync.scan_cache_saved` 报告缓存是否成功保存，不输出对话内容。

2026-10-03 在 SSH Windows 主机既有 Rust 1.98.1 / MSVC 环境验证：

- 修复前两项新增文件占用回归均失败；修复后通过。
- core 全套 1394 项通过，6 项需要外部运行环境的测试忽略。
- 最终 data 全套 205 项、launcher 14 项通过；data 常规轮次忽略 2 项。
  其中扫描基准另行执行通过，另一项需要真实用户源数据。
- 独立扫描基准为 135626989 字节（约 129 MiB）：debug 构建首次扫描 14151 ms，
  缓存命中扫描 1 ms，完整扫描字节数为 0。此结果只代表扫描阶段和该合成文件，
  不代表真实供应商切换或整个 App 启动时间。
- 新增重写与缓存刷新之间正文变化的回归通过；普通启动命中缓存无需完整读取正文，
  跨供应商实际重写时仍支付完整内容核验成本。

## 边界与回滚

第一次启动或大量历史变化仍需要完整扫描；真正跨 provider 切换仍需要安全地
同步必要元数据与索引。消息投影修复仍沿用原有完整证据核验流程。本次扫描基准
不能直接等同于整个 Codex App 的启动时间。

被占用的 rollout 会保留原元数据并稍后重试；现有索引同步继续执行。该情形
不能描述为所有 rollout 都已成功重写。同步本身若出现非占用类错误仍会报告失败。

部署时分别备份 `codex-plus-plus.exe` 和 `codex-plus-plus-manager.exe`，保留启动器
旁的 `codex-plus-plus.runtime-gate.json`。回滚只恢复这两个可执行文件；缓存失效
会自动回退全量扫描，不依赖缓存维持历史可见性。不要替换用户 settings/auth/config。
