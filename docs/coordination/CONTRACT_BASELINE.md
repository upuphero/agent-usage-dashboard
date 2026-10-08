# 接口基线

状态：开发分支契约 **1.3.0** / profile v5 / 应用 **0.0.8**（系统时区跟随，本地实现，尚未经两平台 CI；[设计与本地验证](../ci-validation/follow-system-timezone-v1.md)）。已发布基线为 1.2.0 / profile v4 / 0.0.7，兼容原 1.0/1.1 查询、扫描与设置请求。自动完整扫描已通过 run 37672597908 两平台原生、fixture、安装与出包验证，本机不安装 MSVC/SDK。[本次设计与验证](../ci-validation/auto-full-scan-v1.md)。既有 [0.0.6 / API 1.1 / profile v3 记录](../ci-validation/0.0.6-language-switch.md)保留；真实 GUI/硬件休眠及升级场景仍待验收。

## 归属与交接

| 负责人 | 可修改区域 |
| --- | --- |
| 主 agent | `crates/usage-core`、`crates/usage-contracts`、`apps/desktop`、根构建配置、全局 lockfile、CI、`apps/dashboard/src/api/client.ts` 与 `generated/usage.ts`、本记录和 architecture.md |
| 前端 agent | `apps/dashboard` 中除主 agent 接口文件外的实现与测试；`docs/coordination/frontend.md` |
| 数据 agent | `crates/usage-adapters`、`tests/fixtures`、`scripts/prepare-ccusage.mjs`、`scripts/verify-sidecar.mjs`、`ccusage.lock.json`；`docs/coordination/data.md` |

初始实现按归属交接；不并行改写其他任务尚未完成的实现。两方交付完成后，主 agent 可做先记录影响的必要集成（本次为 Settings 的 transport/Mock/页面），不重写统计规则。新增依赖请在各自 package/crate manifest 中声明，并在交付记录列出；**不要生成或提交全局 lockfile**，由主 agent统一更新。不要提交其他任务未完成的文件。无需创建 GitHub 仓库、推送或发布。

## 唯一来源与接口位置

- Core 领域：`crates/usage-core/src/domain.rs`；端口：`ports.rs`；业务用例：`application.rs`。不依赖 Tauri、SQLite、CLI schema 或真实 I/O。
- API DTO / 稳定错误码 / 扫描状态 / IPC 名：`crates/usage-contracts/src/lib.rs`。不导出数据库行、领域序列化结构或 CLI JSON。
- TS 自动生成：`apps/dashboard/src/api/generated/usage.ts`；禁止手改。
- 生成入口 `scripts/generate-contracts.mjs` 读取 contracts 的受限 Rust schema 宏；Node 即可运行。未知语法/类型拒绝生成；扩展契约需扩展生成器并测试，不另手写 DTO。
- 前端抽象：`apps/dashboard/src/api/client.ts`；前端 agent 实现 MockUsageClient / TauriUsageClient，并在入口注入。页面与 hooks 不调用 invoke。
- 数据 agent 实现 `UsageSource`、`UsageRepository`；进程 runner / SQLite row / migrations 仅存在于 adapters。请公开组装入口并在 data.md 写明签名，主 agent再接入桌面宿主。
- 已接入 product/provider：`claude-code` / `ccusage.claude-code`、`codex` / `ccusage.codex`、`antigravity` / `ccusage.antigravity`。页面按 descriptor 使用 ID，不实现 Provider 统计算法。

## 传输与约定

- `get_api_info`、`list_providers`、`get_settings` 不带参数；其余 command 统一 **`{ request: DTO }`**。get/cancel scan DTO 为 `{ jobId }`。getOverview 的 request 是 OverviewQuery，listSessions 的 request 是 SessionQuery。
- Rust command 返回 `Result<DTO, ApiError>`；transport 将 IPC rejection 归一化成 ApiError。不得按 message 文本判错。
- 使用 `getApiInfo()` 协商版本：客户端只接受 major=1。扫描事件名由生成的 `SCAN_EVENT` 提供；载荷 ScanSummary。轮询是可靠基线；事件可作为刷新提示。
- StartScanRequest 需要 providerId 和 IANA timezone；返回 jobId。同 Provider 重复点击应返回活动任务。queued → running → succeeded/failed/cancelled。终态取消幂等，取消在快照提交前生效；提交完成后显示 succeeded。
- 所有 token **十进制字符串**，USD **十进制定点字符串**；空缺 value=null / accuracy=unavailable，零="0"。knownRows / missingRows 说明聚合覆盖。cost.kind 固定 `api-equivalent-estimate`。
- 完整成功覆盖范围中无日期行可返回 derived 的 total="0"、knownRows=0；未采集缓存与不支持的分项仍为 null。不得在前端用 `value ?? 0` 补零。
- 日期 `YYYY-MM-DD`；范围 `[start,end)`；时间 RFC3339 UTC；IANA timezone。Week 使用 ISO Monday，跨年不按 calendar year 截断。改时区需要重新采集；绝不把 daily 总量重新换时区。
- Overview 只聚合标准 Daily；总计行与模型子行不相加。byModel 仅依据模型明细，缺失模型拆分需要 warning。
- Sessions 是 session 累计量；activeRange 仅表示期间最后活动的会话，不是期间 token。日期×会话用量不支持。offset/limit 分页，limit 1..200；nextOffset=null 表示结束。
- 导出返回不透明 exportId、建议文件名、类型和字节数；不得向 UI 暴露任意路径。JSON 归档保留 dataset/device/scope/revision；CSV 仅用于阅读。
- JSON 导出选定 provider/timezone 的完整标准 Daily/Session 历史，保留完整 scope 身份；范围筛选仅用于 CSV。JSON modelIds 非空返回 UNSUPPORTED_FILTER。宿主使用原生保存对话框，取消返回 CANCELLED。
- 同一 Provider 相同时区的重复 startScan 返回已有 jobId；活动扫描时切换时区返回 SCAN_BUSY，待终态再发起新时区扫描。

## API 1.1 Settings

- getSettings / updateSettings / chooseProviderDirectory 是 UsageClient 的可选扩展；客户端按 settings-read/settings-write/source-directory-selection 能力协商。旧 1.0 服务保持查询/扫描可用，并禁用设置写入。
- SettingsResult 包含十进制 revision、timezone、providers（providerId/enabled/directory）、采集说明。directory 只有 directoryRef 与脱敏 label，没有绝对路径。
- 更新传 `{expectedRevision,timezone,providers:[{providerId,enabled,directoryRef}]}`；null directoryRef 使用默认目录，当前或新选择的引用沿用对应目录。版本冲突返回 SETTINGS_CONFLICT；未知/过期引用返回 INVALID_DIRECTORY_REF。
- 目录选择由宿主原生对话框完成，取消成功返回 directory=null；引用有效期 5 分钟并绑定 Provider，保存后持久化。三个来源开关默认关闭；“启用并扫描”明确启动采集。每个 Provider 有独立持久 dataset，迁移保持 device/已有 Claude 身份，不清空历史。
- 扫描活动中返回 SCAN_BUSY，退出中返回 SHUTTING_DOWN；成功原子持久化后才替换运行期配置。时区改变不重新分桶旧快照。新增错误码集合也由 contracts 生成 ERROR_CODES，前端不再手写另一份列表。

## API 1.2 自动完整扫描

- 能力 `auto-full-scan`：`getAutoCollection()` 无参数，返回配置/revision/统计时区和仅已启用来源的状态（state/jobId/lastSuccessAt/nextCheckAt/watching/error）。时间为 UTC RFC3339，前端按统计时区和界面语言显示；不暴露原生路径/指纹。
- `updateAutoCollection({expectedRevision,config:{enabled,intervalMinutes}})` 独立保存自动配置；默认关闭/5，合法间隔 1/5/15，共享 Settings revision，冲突返回 SETTINGS_CONFLICT，非法间隔 INVALID_QUERY。活动扫描中可写，不接受来源/目录/时区字段，不能绕过原设置门禁。
- `AUTO_COLLECTION_EVENT` 载荷 AutoCollectionStatus；既有 `SCAN_EVENT` 载荷不变。UsageClient 的可选订阅统一处理事件、后备同步和恢复；订阅卸载释放监听/定时器。旧服务 UI 明确不可用且不发新请求。
- 关闭自动采集仅取消调度器拥有的未提交任务并清除 pending，已提交结果保留。手动加入自动任务后关闭开关也不取消该任务。单独取消仍走既有 cancelScan(jobId)。三个 Adapter incremental 能力保持 false。

## API 1.3 系统时区跟随

- 能力 `timezone-follow-system`：`getTimezone()` 无参数，返回 `TimezoneStatus`；`updateTimezone({expectedRevision,mode,timezone})` 独立保存模式。`mode` 为 `follow-system` 或 `fixed`；fixed 必须带有效 IANA 时区（宿主规范化别名），follow-system 必须为 null，否则 INVALID_QUERY。共享 Settings revision，冲突 SETTINGS_CONFLICT；固定模式在活动扫描中更换时区返回 SCAN_BUSY 且不写入；切换为跟随系统可随时保存，检测到的时区在无活动扫描时再应用。
- `TimezoneStatus`：十进制 `sequence`（单调递增；客户端忽略低于已应用值的响应或事件）、`revision`、`mode`、`effectiveTimezone`（查询/扫描唯一使用的时区）、`systemTimezone`（最后一次成功检测）、`detectionError`、`pendingTimezone`（跟随模式下等待安全边界的目标）、`rebuild`（idle/pending/rebuilding/backoff）、`nextRetryAt`（UTC RFC3339）以及仅已启用来源的 `providers`（pending/rebuilding/succeeded/failed、jobId、error）。`TIMEZONE_EVENT = usage://timezone-updated` 载荷相同，内容不变时不重复发送。
- 生效时区变化（系统检测或固定选择）时 `settingsRevision+1`，持久标记需要重建；从原始日志重新生成该时区的 Daily/Session 快照，不重新分桶已有 Daily，旧时区快照保留。只有当前目标时区的成功扫描能确认对应来源；重建期间更换某来源的目录会撤销该来源的确认，须按新目录重新采集。
- 旧 `updateSettings` 不变：`timezone` 与生效时区相同（含别名）时不改变模式；不同则表示固定为该时区并触发重建。旧服务不提供该能力，新客户端不发送新命令/参数，并回退到原时区选择器。profile v5 增加 `timezoneMode`；v1–v4 迁移为 fixed 并保留已保存时区，旧宿主拒绝读取 v5。

## 数据端必须遵循

- 快照 identity 是结构化 `SnapshotKey`；revision 不进入 key，token/cost/parser 版本不进入 key。`QueryScope::Standard` 是唯一权威历史视图，filtered 缓存不参与全局总计。
- 每个 Day / Session `RowKey` 唯一，modelId=None 是父总行。完整成功 batch 一次事务替换全部同 key 行并递增 revision；删除旧模型明细，不执行累加。
- `CollectionBatch` 不能空（空 Vec 不证明来源可读）；成功无数据用带 complete coverage 的空行快照表示。缺文件/历史轮转/局部覆盖须返回 CoverageIncomplete，保留旧快照。第一阶段不自动合并 partial 历史。
- Core 额外保护：正常扫描中，标准快照的已有 Day/Session 维度消失时，整批返回 CoverageIncomplete 并保留旧数据；同一维度内模型拆分和数值修正仍可替换。显式重建/清除历史入口本阶段未实现。
- 跨 Daily / Session 命令记录开始/结束时间，不宣称一致读取；保留允许的 warning code，不保存原始输出、会话标题、正文或绝对路径。
- `commit_batch` 原子性由 repository 实现；同 key 来自不同 provider/origin 必须拒绝 DatasetConflict。Core 校验 batch，repository 独立保护原子性与唯一性。
- `save_scan` 按 jobId upsert，listScans 返回该 provider 历史状态，错误仅 CoreError。Clock 生产实现放宿主；测试固定时间。
- 固定 ccusage 版本与两目标校验，取消/超时/输出限制/退出回收由 runner 实现。Windows 隐藏子进程窗口。path_hint 必须脱敏。

## 构建与测试

在项目根执行：

```text
pnpm install
pnpm dev                       # 普通浏览器，前端 agent 完成 Mock 后独立运行
pnpm typecheck
pnpm lint
pnpm test
pnpm build
cargo test -p usage-core -p usage-contracts -p usage-adapters --locked
cargo fmt --all --check
cargo clippy -p usage-core -p usage-contracts -p usage-adapters --all-targets --locked -- -D warnings
pnpm contracts:generate
pnpm contracts:check
pnpm boundaries:check
pnpm version:check
```

`cargo test` 默认排除桌面主机，Ubuntu 的纯 Core 检查不要求 WebKit 或 sidecar。桌面本地无 sidecar 验证使用 tauri.local.conf.json；发行构建必须准备真实、锁定且验证过的本目标 sidecar，禁止用占位程序冒充。macOS 在本机不可执行；现已由免费 macos-15 ARM64 runner 实际完成编译、DMG/安装后的 sidecar/native 验证。Windows 同样由 windows-2022 x64 runner 验证；本机 MSVC/SDK 不安装。

## 当前待办与变更流程

当前已完成 Settings 读写/目录选择、scan/auto 事件订阅及 API 1.2 自动配置/状态，不能再作为缺失接口排期。实现/验证结论以 0.0.7 最终记录为准：67 前端、16 Node/SQL、43 Linux Rust、两平台各 75 默认和 5 native（安装后再跑 5）、严格 clippy 与集中出包均通过。

真正剩余的接口工作对应 [T7–T15](TODO_GUIDE.md#t7)：增量、用户数据维护/导入、多设备/独立数据集、时区跟随、日期/项目、桌面体验与来源/quota。T1–T3 是现有接口的真实平台验收，不是重新实现相同功能。当前订阅传递任务状态与自动状态，不宣称已提供逐文件百分比；来源卡片的 collector/normalization 版本展示仍属于可选诊断扩展。

破坏性变更先记录原因/迁移与新版本，修改 Rust 契约并重新生成 TS，再跑相应兼容/边界/业务/平台检查；不把原始数据库行或日志正文加入 UI DTO。发布/签名范围见 [T4–T6](TODO_GUIDE.md#t4)，此次文档整理不执行新 Release 或凭据操作。

## 早期集成与验证历史（2026-10-04）

下文 38/51 项测试和 API 1.1 的“最新”是当时记录，保留语义交接事实；当前版本/证据以本文开头与 0.0.7 记录为准。

主 agent：Core 业务和内存测试、DTO 映射、薄 commands、任务生命周期、依赖组装、导出、构建/CI；前端：Dashboard/Mock/transport/组件测试；数据：Claude Code parser/runner/SQLite/fixture/sidecar 锁。

主 agent 上述业务/宿主/导出/构建代码已落地，已读取两份交付并完成真实 SQLite/Claude adapter 注入及 Settings 集成。前端 38 项、数据/主脚本 16 项、各平台 51 项 Rust/native tests、安装后重复的 3 项 native tests 与严格 clippy 已实际通过。真实 UI/IPC 交互仍待验收。接口基线已就绪，前端和数据均已交付并完成集成；文件及验证详情见 architecture.md。

SessionPage 的 total/分页按 dataset/session 唯一会话计数，Core 合并模型明细时保留 missingRows；日期过滤用整个 session 的最后活动时间。API DTO 不变，Core 查询结果 SessionEntry 为业务视图（不再将 ReportRow 直接带给宿主）；Provider/Repository 端口及采集 ReportRow 不变。

2026-10-04 最新验证：[run 37251178175](https://github.com/upuphero/agent-usage-dashboard/actions/runs/37251178175) 的五个 jobs 全部通过，代码 commit fae5666d3c74688fa38fefca6a90ee703bf1974f。Windows NSIS 3.95 MiB / Mac ARM64 DMG 4.93 MiB；安装后原始 sidecar SHA、Unicode/空格路径、fixture 和 native tests 实际通过。集中安装器/证据/SHA256SUMS 保留一天，未发布正式 Release。API 1.1.0 不变；详见 [验收证据](../ci-validation/2026-10-04.md)。真实 UI/最低 OS/正式签名/完整 notices 保持待办。

破坏性接口调整先在 architecture.md 记录原因、影响、迁移和新版本，再统一改契约及生成结果；不得无说明改变已交接语义。各 agent 在自己的交付记录列出实装入口、验证命令/结果、限制和待办。主 agent读取这些记录后集成。
