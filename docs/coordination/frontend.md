# 前端交付记录

## 当前前端状态（2026-10-07）

应用 **0.0.7**，消费生成的 **API 1.2**。67 项前端 tests、typecheck/lint/build、契约/依赖边界和浏览器合成交互验收通过。[最终证据](../ci-validation/auto-full-scan-v1.md) · [当前待办](REMAINING_WORK.md)

- Overview/Providers/Sessions/Settings、统计筛选、字段质量与完整 JSON/筛选 CSV 已接入；中文默认/英文切换、显示偏好与本地统计时区保留。
- Settings 的持久来源开关、目录选择、不透明引用、revision 校验已实现，不再是只读占位。真实桌面原生选择器与完整 GUI/IPC 操作仍待 T1 验收。
- AutomaticCollection 设置、1/5/15（默认关闭/5）、状态/时间、任务区分和按 job ID 取消已接入 Tauri 与合成 Mock；旧服务缺能力时明确不可用。
- UsageClient.subscribeUsage 已接入扫描/自动状态事件；transport 负责监听释放、失败重试、focus/visibility 和 15 秒后备同步。扫描开始/结束刷新来源/图表/会话，纯自动状态事件只更新状态缓存；getScan 的手动长轮询仍保留。
- 普通页面/hooks 不放完整扫描定时器；真正采集与调度在 Rust。Mock 仅当前页面会话合成行为，不读取用户磁盘，不代表实际后端/真机验收。

剩余前端工作：T1–T3 真机验收；T8–T12 数据维护/导入/时区跟随/日期项目/桌面体验 UI。已有事件订阅不再列为“缺少订阅接口”；采集器版本展示和更细处理进度仍属于可选诊断扩展。

## 首阶段历史交付（2026-10-04）

以下 32 项测试、API 1.0、只读 Settings、未订阅事件以及当时开发服务器状态均为原始交付快照，已被后续集成更新；不是当前能力或当前仍在运行的服务声明。

状态：**第一阶段前端已实现；类型检查、lint、32 项测试、生产构建及实际浏览器检查通过。真实桌面/采集链路尚未验收。** 更新时间：2026-10-04。

## 接口与文件归属

- 已读取开发计划、用户 AGENTS 指示、CONTRACT_BASELINE.md；首次检查只有计划，随后主 agent 已建立 API 1.0.0 / workspace 基线，前置缺口已解除。
- 直接消费 `src/api/client.ts` 与 `src/api/generated/usage.ts`，未修改公共接口、生成 DTO、crates、desktop、根构建配置或全局 lockfile。未新建 agent、提交、推送或发布。
- 实现范围均在 `apps/dashboard`；本记录为唯一维护的协作交付文件。

## 实装入口

- `src/main.tsx` / `app/createClient.ts`：React、QueryClient、UsageClient 注入。普通浏览器默认 Mock；桌面 bridge 存在时选择 Tauri；`?client=mock` 强制演示，`?client=tauri` 强制真实 transport，浏览器无 bridge 时明确报错，绝不回退伪造数据。
- `app/App.tsx`：Overview / Providers / Sessions / Settings、日期/来源/模型/趋势粒度、错误与缓存刷新、全局扫描状态、主题。
- `features/useUsage.ts`：只通过注入的 UsageClient 获取业务数据。TanStack Query 缓存，失败保留旧结果；扫描终态后失效 Overview、Providers、Sessions 缓存。
- `api/transports/mock/MockUsageClient.ts`：显式演示数据、预置统计结果、正常/partial/stale/empty/error/loading/scan-error/limited/version-mismatch 场景、扫描合并和取消、分页、导出结果模拟。未读取日志、数据库或真实账号。
- `api/transports/tauri/TauriUsageClient.ts`：generated COMMANDS、精确 `{ request: DTO }`、API major 协商、稳定 ApiError 归一化、轮询与取消。Tauri SDK 和 bridge 检测仅在 `api/transports/tauri`。
- `api/transports/scan.ts`：两 transport 的扫描等待。`getScan` 在客户端内轮询到终态，保持公开方法与 wire DTO；UI 等待期间显示任务进行中，Provider.lastScan 仍展示后端即时 queued/running 快照。最多 800 次读取、Tauri 间隔 750ms；等待超时保留 jobId，可继续等待/取消，不自动取消后端任务。未使用扫描事件，轮询为基线。

## 行为与限制

- Token 十进制字符串用 BigInt 格式化；null 显示不可用，`"0"` 显示零。仅做展示和图表几何缩放，不归一化、去重、加总 token 或计算费用。
- 成本固定标为「API 等价估算成本」/ USD，与订阅账单区分；展示字段精度、knownRows/missingRows、未估价模型、未知价格日期、coverage、stale、最后更新时间。推理仅作为输出子集展示。
- 能力不足时禁用模型筛选、会话报表和日期×会话期间 token。Sessions 可按最后活动期间筛选，但始终展示累计量，不推导期间用量。
- 全部来源的 Sessions 仅请求支持 session 的来源，并展示排除说明。Provider 的能力与状态按服务 descriptor 渲染，不根据品牌推断正式支持。
- Mock 固定日期 2026-09-28..2026-10-04 / America/Phoenix，提供今日、本周、本月预置结果；不支持任意日期或其他时区的演示统计。模型 `demo-*`、设备/数据集 `demo-*` 均为合成标识。
- Settings 主题可本地保存；真实模式时区仅影响查询并提示需重扫，演示时区固定。契约没有 Settings 写入/路径选择/维护命令，来源开关、路径、数据维护、开机启动如实禁用，不假装保存。
- JSON 标为所选来源/时区的完整历史归档，模型筛选时禁用；CSV 使用当前查询。Mock 导出明确不会保存文件；真实导出依赖宿主原生保存对话框。
- 没有新图表/UI/测试依赖，使用现有 React、TanStack Query、Vitest 与 React DOM。CSS/SVG 图表无需加载外部字体或图片。
- 模型选项从无模型筛选的独立查询缓存读取，加载新结果期间保留当前模型标签；会话筛选使用会话来源能力，避免向不支持模型的每日来源发送交叉筛选。
- dashboard test 脚本已改为 `vitest run`，没有测试文件时会失败，不继续接受空测试绿灯。依赖及版本未改变。

## 启动与场景

```powershell
# 项目根，使用主 agent 安装并锁定的 pnpm 9
pnpm --filter @usage/dashboard dev
# 当前工具环境可用的等价启动命令：
# & '%APPDATA%\npm\pnpm.cmd' --filter @usage/dashboard dev
# http://127.0.0.1:1420/  浏览器演示
# /#overview  /#providers  /#sessions  /#settings
# /?scenario=stale  /?scenario=empty  /?scenario=error
# /?scenario=loading  /?scenario=scan-error  /?scenario=limited
# /?scenario=version-mismatch  /?client=tauri
```

当前工具环境 PATH 的 bundled pnpm 在最初两次检查时先尝试重新安装整个 workspace，因无 TTY 退出；随后使用现有 `%APPDATA%\npm\pnpm.cmd`（pnpm 9）运行全部检查。没有主动执行 install 或生成 lockfile。pnpm-lock.yaml 保持 84,256 字节、原始修改时间 2026-10-04 15:00:16，前后校验 SHA-256 相同。

Vite 开发服务器目前保持运行，仅监听 `127.0.0.1:1420`，浏览器已打开默认 Overview 演示页；无需 Rust / SQLite / ccusage。

## 验证结果

最后一次完整前端检查：2026-10-04 16:22（America/Phoenix）；筛选修正之后重新执行。

| 检查 | 实际结果 |
| --- | --- |
| `pnpm --filter @usage/dashboard typecheck` | 通过，直接消费 API 1.0.0 生成契约 |
| `pnpm --filter @usage/dashboard lint` | 通过，Tauri SDK 限制生效 |
| `pnpm --filter @usage/dashboard test` | **4 文件、32 项通过**：协议 3、Mock/Tauri 契约及场景 16、组件渲染/格式/日期 9、入口注入 4 |
| `pnpm --filter @usage/dashboard build` | 通过，95 modules；JS 273.01 kB / gzip 85.64 kB，CSS 19.95 kB / gzip 4.87 kB，HTML 0.63 kB；没有外部字体、图片或图表库 |
| `node scripts/generate-contracts.mjs --check` | 通过，生成 DTO 无漂移 |
| `node scripts/check-boundaries.mjs` | 通过，SDK/invoke 仅存在于 tauri transport；未改业务核心或宿主 |
| 普通浏览器独立运行 | **通过**，Codex in-app Chromium 浏览器实际打开 localhost；只启动 Vite |
| 默认窗口 / 390px 窄窗口 | 截图实际检查；Overview 布局正常，窄窗口 documentWidth 与 clientWidth 均为 375px（滚动条占用 15px），没有页面级横向溢出；随后恢复默认视口 |
| 浏览器控制台 | 已读取 warn/error 日志，没有警告或错误 |
| Rust / 真实 Tauri IPC / SQLite / ccusage / 安装包 | **未执行、未验收**；Tauri transport 单测使用注入的模拟 command invoker，不能视为真实链路成功 |
| macOS Apple Silicon ARM64 / WKWebView | **未验证**；当前只有 Windows 执行环境 |

组件测试使用现有 React DOM 的服务端渲染检查文本、状态、禁用属性及精度，不冒充 DOM 交互测试。真实交互由本次实际浏览器操作验证，未新增 jsdom 或 Testing Library 依赖。

浏览器已实际检查以下行为：

- 四个导航页面；日期/来源/模型筛选；模型选项加载后保留；模型筛选时 JSON 归档禁用。
- 深浅主题切换；演示 CSV 导出成功提示明确「未保存文件」；Settings 未开放写入的操作禁用。
- 扫描进行中、成功、取消；scan-error 的 COLLECTION_FAILED、stale 提示与原 1,480,000 token 保留。
- empty 的已知 total=0 / 其他缺失字段不可用；PERMISSION_DENIED 场景不产生指标卡或伪造零；partial、stale、loading 显示后成功加载。
- limited 场景模型禁用、会话报表不可用；API_VERSION_UNSUPPORTED 阻断业务页面。
- `?client=tauri` 在无 bridge 的浏览器明确显示桌面连接不可用，没有演示数据回退。
- 今日活跃会话筛选仅剩 demo-session-01，但保留其全会话累计 820,000 token；展开元数据时开始时间显示「来源未提供」、设备/数据集为显式 demo ID。

开发检查中曾因短演示扫描已完成而错过取消按钮，随后在任务仍活动时重新验证取消成功；loading 场景曾超过单次浏览器定位等待期限，之后实际确认其完成加载且控制台无错误。以上为检查时序问题，未将尝试当作验证通过。

## 主 agent 集成事项

- 无新增依赖需求，无 lockfile 更新请求。
- 本地组件/模拟 IPC 测试只验证前端消费契约，不代表 Rust、真实 IPC、SQLite、ccusage 或安装包链路通过。当前宿主 API 的 `adapter-integration-pending` 会在页面明确提示。
- 按契约公共方法目前只能轮询，没有订阅/瞬时扫描状态回调；本实现的 getScan 等待终态。若未来需要精细实时进度，建议由主 agent 统一增加 subscribeScan/watchScan 公共接口，不应让页面引入 Tauri 事件。
- Settings 后续需由主 agent 定义读取/更新设置、来源配置、目录选择与维护的 DTO/命令。当前 provider 没有 collectorVersion/normalizationVersion，Overview 只显示已有 pricingVersions；不虚构缺失版本信息。
- 实际 Tauri 启动、真实采集扫描、导出保存与 macOS ARM64 验证待主 agent 在具备环境后执行。

后续契约请求（**仅记录需求，未创建第二套 DTO 或接口**）：

| 原因 | 建议由主 agent 统一定义的字段/能力 | 影响 |
| --- | --- | --- |
| Settings 缺少读写配置与目录选择 | 统计 timezone、按 providerId 的 enabled、宿主选择并校验的来源目录引用；对应 settings-read/settings-write/目录选择能力 | Core/宿主需校验和持久化，生成 DTO 后前端才启用开关；theme 仍可保持本地显示偏好 |
| 当前契约未向前端提供采集/归一化版本 | Provider/Overview 可选 collectorVersion、normalizationVersion 或按来源的版本元数据 | 以兼容可选扩展方式生成，前端展示真实版本；缺失仍为未知 |
| 细粒度实时扫描状态没有统一订阅入口 | 基于现有 ScanSummary 的客户端订阅及释放机制；jobId/state/error 沿用同一 DTO | 定时器/事件/清理继续在 transport，页面消费统一状态，不引入 Tauri SDK |

上述 Settings 与实时进度扩展不作为本阶段已实现功能宣传；首阶段已有可用的扫描/取消/查询闭环及准确的不可用说明。
