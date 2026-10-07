# 自动完整扫描第一版：本地设计与验证

日期：America/Phoenix 2026-10-05。基线应用 0.0.6；用户授权提交、推送及 CI 验证/出包后，新包版本升级为 0.0.7，API 1.2.0 / profile v4。遵循 [开发计划](../coordination/AUTO_FULL_SCAN_PLAN.md)。本次 CI 结果在运行完成后记录；既有版本的 CI 成功记录不能作为本次验证证据。

## 设计与边界

- 配置：profile 的 `autoCollection` 保存 `enabled`、`intervalMinutes`；旧配置默认关闭/5 分钟，合法值只有 1/5/15。v1–v3 升至 v4，保留 deviceId、datasetId、历史、本地时区和 UTC 迁移标记。v3 的已保存 UTC 不再二次迁移。旧宿主拒绝 v4，避免降级覆盖配置。
- 接口：`auto-full-scan` 能力对应 `getAutoCollection`、`updateAutoCollection` 及 `AUTO_COLLECTION_EVENT`。独立更新请求只接受 `{expectedRevision,config:{enabled,intervalMinutes}}`，共享 settings revision；扫描中可以保存自动设置，但无法顺带改目录/来源/时区。原查询、扫描、设置 DTO 未添加旧服务不认识的参数；旧服务的自动 UI 明确不可用。所有新增 DTO/事件从 Rust 契约生成。
- Core 端口：`UsageSource::inspect(CollectRequest,CancellationToken)` 返回原生内部 `SourceObservation`，`watch(SourceConfig,ChangeCallback)` 返回 RAII `SourceWatch`。默认实现不支持；三个真实 Adapter 实现 IO，宿主不猜默认目录。路径、正文和指纹均不传给前端。
- 范围：Adapter 原有根目录解析和共享 `changes::directories` 同时服务采集/检查。Claude 为 projects JSONL；Codex 为 sessions/archived_sessions（保留原 fallback）；Antigravity 为 conversations 的 .db 和对应 -wal。.pb 仅检查元数据以提示边界变动，仍不能采集。忽略 auth、临时采集输出、shm/锁文件、纯访问和无关属性事件。
- 指纹：Provider、dataset、解析后的根目录、统计时区、collector/normalization 版本划分 scope；文件集合、路径、原生身份（Windows volume/file index，Unix dev/inode）、大小、mtime 组成元数据指纹。监听内容事件、丢失/溢出/未知事件强制复核，即使元数据未变；监听不可用时保守按间隔完整复核，避免遗漏同大小/同 mtime 的改写。
- 政策：纯 `scheduler/policy.rs` 接收可控单调秒数，2 秒合并窗口、每来源一个 pending、按配置间隔限流，失败退避 60→120→…→900 秒（且不短于配置间隔）。取消等待至少一个检查间隔。内存成功基线只在提交成功时推进，使用扫描前指纹；扫描中新事件通过 generation 保留 dirty。启动/重新启用必做一次扫描。
- 并发：Rust Scheduler 自动任务全局串行，未扫描/最久未启动的来源优先，避免长期写入的慢来源占满每个扫描机会；冷却期间保留 dirty，避免每 2 秒重复遍历元数据。Runtime 在同一活动锁内核对最新开关/配置与预检查范围，拒绝加入已经运行的手动任务。手动扫描继续原入口/合并规则；用户加入自动任务后，该任务不再被“关闭自动采集”取消。已提交结果沿用原取消/提交边界。来源/时区更新沿用活动扫描门禁。
- 生命周期：Tokio tick 使用 Skip；独立的 1 秒 pulse 检测单调 tick 间隔或 UTC 跨度超过 10 秒，合并一次保守恢复检查，耗时目录检查不冒充休眠。UTC 仅用于恢复提示/展示，去抖、限流和退避均用单调时间；扫描限流从实际 start_scan 返回时刻向上取整记录，目录检查/存储耗时不能缩短间隔。两平台不依赖 React 扫描定时器、不补 tick、不增加后台服务。退出先停止两个原生 timer、调度入队与监听，再由原 Runtime 取消/回收任务；不隐式恢复旧进程未完成扫描。
- UI：一次 UsageClient 订阅封装原生扫描/自动状态事件，15 秒 transport 后备同步、focus/visibility 恢复同步与失败订阅重试。扫描开始/终止失效来源/overview/sessions 缓存；纯状态事件只更新自动状态缓存，避免刷新循环。中文/英文均有设置、状态、错误、取消与可访问性标签，UTC 时间按当前统计时区展示。

本次继续原有 Daily/Session 完整快照替换。三个 `supportsIncrementalCollection` 均为 false。token、价格、数据集身份、业务聚合、SQLite schema 和快照替换规则未改。

## 设置行为

| 操作 | 行为 |
| --- | --- |
| 开启 / 应用启动且已开启 | 清空进程内成功基线，已启用来源串行完整扫描一次 |
| 关闭自动采集 | 清除自动 pending、停止检查/监听、取消仍属于调度器的未提交任务；已提交结果与手动任务保留 |
| 单独取消自动任务 | 既有 job ID 取消接口；不推进基线、不清空历史，等下一轮检查 |
| 关闭来源 | 无活动扫描时保存；停止该来源自动工作，历史保留 |
| 改变间隔 | 可以扫描中保存，后续间隔/限流采用新值，不重置成功基线 |
| 改目录 / 统计时区 | 保留扫描门禁；保存后自动范围重新建立，活动任务不受配置替换污染 |
| 切换界面语言 | 只改变文案与格式，不触发扫描，不改统计时区/自动配置 |
| 重启 | 持久配置保留，未完成任务按原规则标记取消；自动新进程保守重扫 |

## 本地已通过

| 验证 | 结果 |
| --- | --- |
| TypeScript / ESLint | 通过，使用项目现有 tsc/eslint |
| Vitest | 12 个文件、67 项通过（原 57 + 自动 transport 7 + UI/cache 3） |
| Node scripts | 10 项通过；生成器、command registry、既有构建/产物门槛 |
| Adapter Node / SQL | 6 项通过 |
| contracts / boundaries / version | 通过，应用版本仍为 0.0.6 |
| cargo fmt / diff whitespace | 通过 |
| 生产前端 build | 通过（最终代码 tsc + Vite，103 modules，JS 332.22 kB / gzip 105.45 kB；不等于桌面安装包） |
| 浏览器合成验收 | 默认关闭/5；1 和 15 保存；重新启用后后台任务出现；成功时间无需刷新更新；按 job ID 取消后任务消失；中英文切换保持已保存开关、1 分钟及 America/Phoenix |

前端 fake timers 验证初次扫描、成批事件合并、扫描中写入在后续轮次保留、干净来源跳过、自动/手动任务归属、失败保留合成结果和冷却、旧服务不发送新参数、监听重连/卸载清理；缓存测试验证未由 UI 发起的终态会刷新来源/图表/会话，而状态事件不产生缓存循环。测试不读取用户日志、不等待真实分钟。

本机 pnpm fallback 与既有 node_modules 管理版本不一致，直接运行对应 package scripts 所用的现有二进制；未修改 npm 依赖或 lockfile。notify 8.2.0 通过 crates.io 下载并锁定，只增加其必要 Rust 依赖。

## 未验证与两平台出口

本机 `cargo test -p usage-core -p usage-contracts -p usage-adapters --locked --offline` 在 build script 阶段失败：**link.exe not found**。遵循计划未安装 MSVC/SDK。Rust 测试/clippy、host 编译、两平台 native fixture、新安装/升级包、本机真实 Tauri UI、真实硬件休眠均不能标成通过。

新增待 CI 运行的 Rust 检查：profile v3 默认/身份/时区保持；自动配置合法值/revision/持久化；Runtime 手动优先、自动开关独立门禁/取消归属；可控时间的去抖、连续写入限流、扫描中 dirty、失败/取消基线与恢复；三来源 metadata/历史修正/身份替换/WAL 和监听访问噪声过滤；原生合成文件监听/RAII 退出；实际锁定 sidecar 的自动 Scheduler 与手动完整快照一致性 fixture。元数据检查有 30 秒超时及协作取消，超时按失败保留基线/历史并退避。

本次用户已明确授权提交、推送并运行现有 Windows/macOS CI 验证和生成安装包。沿用 package.yml：Linux core/adapters unit + clippy；Windows x64 / macOS ARM64 host unit、ignored sidecar fixture、host clippy，NSIS/portable ZIP/DMG 安装后重复 fixture。未改旧 tag/Release、工作流或签名凭据。

两平台实际恢复验证方式（待执行）：安装新包，配置合成日志并开启 1 分钟，记录最近成功/下次检查；最小化后追加 JSONL 或对支持的合成数据库写入 WAL，确认后台完成并更新图表；Windows 使用系统“睡眠”，macOS 使用 Apple 菜单“睡眠”，等待跨过至少三个周期，在休眠前/恢复后修正合成历史，唤醒应只合并一次检查、不补三个扫描；连续写入不得每 2 秒完整扫描；取消/关闭后不立即重启该来源；退出后确认主进程/sidecar 无残留，重启配置/身份/历史保留。UTC 跨度检测的合成测试仅证明政策，不代替这两次真实睡眠验证。

浏览器截图（合成数据，本地未跟踪 artifact）：`artifacts/browser-validation/auto-full-scan-zh.jpg`。

## 2026-10-07 CI 续验

运行 [37419753551](https://github.com/upuphero/agent-usage-dashboard/actions/runs/37419753551)，提交 `f663ee331f01ffa1153b20131c83aea1e247d8ea`：基础前端/Core/Adapter 验证通过；Windows 原生 unit、sidecar fixture、NSIS/portable 安装检查和 host clippy 全部通过。macOS 构建成功，但原生文件监听测试超时，因此没有生成集中安装包。

已定位并修正监听路径别名问题：FSEvents 的事件路径经过规范化，而原过滤器可保留 `/var` 等别名，导致合法事件不匹配。监听注册与过滤现统一使用 canonical roots；同一原生测试在 Unix 显式使用目录别名，避免只修改 fixture 来绕过问题。修复后重新运行完整两平台流水线，最终结果与安装包校验在完成后补充。
