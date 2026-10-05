# 主 agent 架构与集成交付

2026-10-04：读取开发计划 V0.2.2 和用户 AGENTS 指示；项目原先仅有计划。建立 Cargo/pnpm workspace、三个 crate、独立 Dashboard package、Tauri 2 宿主骨架。API 1.0.0 唯一来源 usage-contracts；接口位置、文件归属和任务分工见 CONTRACT_BASELINE.md。

核心选择：规范化快照与 API DTO 分开；Core 仅依赖端口；标准 Daily/Session 分离；拒绝不完整 batch 的破坏性替换；token IPC 使用字符串；字段未知返回 null；ISO 周；查询不重分桶已聚合日期。平台仅 x86_64-pc-windows-msvc 和 aarch64-apple-darwin。

首次依赖安装需要写用户工具缓存及访问 registry，已通过工具的本地开发授权流程执行；不涉及公开仓库、账号凭据或发布。检查结果见下方表格。

数据集副本/重叠多设备数据不在本阶段自动合并；不同未知身份数据集可能重叠，后续导入前必须确认来源。第一阶段不推导不存在的日期×会话数据。最低 OS 配置是开发暂定值，不能宣称已在最低系统实测。

接口基线冻结前调整：本机缺 MSVC 和 Windows SDK，用户明确不安装系统构建工具。现有 WSL 也无 Rust/C 编译器。因此 TS 生成改为 Node 读取 usage-contracts 的受限 `dto!` / `enumeration!` 声明 DSL；Rust serde DTO 和生成类型共享同一声明，无手写 TS 副本。未知字段语法/类型会失败，Node 单测验证 nullable/rename/错误码和失败策略。移除原 ts-rs generator；pnpm contracts:generate / contracts:check 现不需要 Rust。后续扩展 DSL 必须修改生成器并测试。

工具链：Rust 1.91.1、Node 24.19.0、pnpm 9.15.0（本机常规 shell 现有版本）；按 Cargo.lock/pnpm-lock.yaml 固定依赖。Windows Rust 编译和 Core/IPC 测试尚阻塞；不能将格式化检查视为编译通过。

基线后契约扩展（向后兼容，不改变 UsageClient / 请求）：增加 JSON 归档 DTO，唯一来源仍在 contracts。JSON 导出保留所选 Provider/时区的完整标准 Daily/Session 快照以保持 scope/revision，日期仅适用于 CSV 报表；modelIds 非空的 JSON 请求返回 UNSUPPORTED_FILTER，避免裁剪归档导致身份失真。前端需在归档入口说明“完整历史归档”。导出由宿主原生保存对话框选择目标，API 仍只返回不透明引用，不接收路径。此扩展只增加生成类型，数据端端口无需变更。

成本类型收紧为 `CostKind::ApiEquivalentEstimate`，线上值仍为同一个字符串 `api-equivalent-estimate`，前端已有正确字面量无需修改。Core 校验所有可用互斥桶之和与可信 total 一致；无明细的模型交叉筛选返回 UNSUPPORTED_FILTER。warning 只能是 1..64 位大写 ASCII/数字/下划线代码，不接收任意 stderr 文本。Filtered scope 的日期需规范 YYYY-MM-DD，modelIds 按字典顺序去重。上述语义属于原有基线的校验要求。

## 主 agent 实装内容

- Core：检测/Provider 状态、扫描采集/校验/提交、Daily Overview 与 day/ISO week/month 聚合、模型父子行权威选择、Session 活动日期筛选与稳定分页。Clock/Source/Repository 注入，无文件/进程/数据库调用。
- 快照：结构化 identity、标准视图与过滤缓存分离、重复行拒绝、完整 batch 原子替换；内存实现按 key 递增 revision，冲突/溢出回滚整个 batch。失败/取消/局部覆盖不清空历史。
- 精度：字段级 exact/derived/estimated/unavailable、已知部分/缺失行、u64→API 十进制字符串、固定精度 USD；reasoning 是 output 子集，不再次计入 total。已证明完整但无对应日期行的快照可以表示 total=0；未采集缓存、未知字段仍为 null，成功空范围的 knownRows=0。
- 宿主：8 个薄业务 commands、显式 DTO 映射、按 Provider 合并相同时区扫描、不同活动时区返回 SCAN_BUSY、幂等取消、轮询/事件提示、10 秒退出回收窗口。Runner 必须在取消时终止/等待子进程，并在 future drop 时清理进程；最终端到端回收待数据 runner 验证。
- 导出：Core 结果写 CSV（安全引号/公式转义），完整规范化标准快照写 JSON 归档，保留 dataset/device/revision/精度/覆盖/采集版本。由 Rust 调用 Tauri 原生保存对话框；网页无任意路径/文件或 shell 权限。参照 [Tauri dialog Rust API](https://v2.tauri.app/plugin/dialog/)。
- 构建：根 package 与 Cargo/pnpm locks、版本/契约/依赖边界检查、两目标 build-desktop 脚本、体积/哈希汇总脚本、Windows ICO/macOS ICNS 资源。所有 ccusage 准备/验证/锁文件仍归数据 agent，缺失时完整打包失败。
- CI：Ubuntu 纯 Core/契约/前端验证；Windows x64 与 macOS ARM64 宿主矩阵；手动 unsigned/ad-hoc 测试包 workflow。Actions 固定经官方 GitHub API 核实的完整 SHA。不创建或发布 Release，不配置签名凭据；CI 尚未实际运行。

## 验证记录

| 检查 | 实际结果 |
| --- | --- |
| pnpm install --frozen-lockfile | 通过；pnpm 9.15.0 |
| pnpm typecheck / lint | 通过；归档 DTO 扩展后复核通过 |
| pnpm build | 采用 frontend.md 的最新有效构建：95 modules，JS 273.01 kB / CSS 19.95 kB；主 agent 未修改前端实现 |
| pnpm test | 前端交付后主 agent 复验：4 文件、32 项通过；包含注入的模拟 IPC，不是实际 Tauri IPC |
| API 1.1 Settings 增量 | 5 文件、38 项通过（原 32 + 设置协议/冲突/引用/关闭保留历史/扫描写入限制/组件能力）；typecheck / lint 通过 |
| Settings 生产构建 | 主 agent 本次实际通过：95 modules，JS 279.94 kB / gzip 87.69 kB，CSS 19.95 kB；无新增前端依赖 |
| node --test scripts/*.test.mjs crates/usage-adapters/tests/*.test.mjs | 主 agent 复验 14 项通过（8 个构建/契约/格式检查 + 6 个数据归档/实际 SQL schema 检查） |
| contracts:check / boundaries:check / version:check | 通过 |
| 两份 workflow YAML / target matrix / permissions / Action SHA 结构检查 | 使用现有 js-yaml 依赖检查通过；workflow 本身尚未执行 |
| cargo fmt --all --check | 通过 |
| cargo check -p usage-contracts --locked --offline | 阻塞：link.exe 不存在（MSVC/SDK 缺失） |
| Core/契约/宿主/Adapter Rust 测试源码及 native 合成测试入口 | 已写；未执行通过，clippy 未验证；不能把 Node SQLite schema 测试称为 Rust repository 通过 |
| Tauri info | CLI 能解析配置；检测到 WebView2，明确报告缺 MSVC/SDK；不等同于 build 通过 |
| Windows ccusage 20.0.26 / 4,222,976 bytes | 主 agent 已按数据锁准备到桌面 binaries 目录并复验所有 CLI fixture；无真实用户日志读取 |
| prepare-notices --check | 已通过；从锁定 SRI tarball 校验完整 ccusage MIT 文本，其他依赖库存待补 |
| Windows exe/NSIS 与安装、包内 sidecar | 未验证；本机缺 MSVC/SDK |
| macOS ARM64 app/DMG/ad-hoc/最低系统/安装 | 未验证；当前无 Mac 执行环境 |

## 下一次集成入口

2026-10-04 继续开发，先记录本次影响：两份任务交付仍未出现，继续只修改主 agent 文件。API 1.0.0 / UsageClient / UsageSource 与 UsageRepository 方法签名保持不变。

- Session 查询改为每个 dataset/session 一个结果；缺父总行时在 Core 合并模型明细，保留字段 missingRows，过滤日期使用整个会话的最后活动时间，展示仍是累计用量。Core 查询结果 SessionEntry 不再携带数据库报表行，宿主直接映射其业务聚合；数据采集 ReportRow 不变。
- CancellationToken 保持现有 cancel/check/is_cancelled 接口，内部增加原子提交阶段；取消与 begin_commit 的先后形成明确边界。新 commit_started 仅供宿主判断异常退出状态，Provider 无须修改。
- 宿主捕获异步任务 panic，恢复上次进程遗留的 queued/running 扫描，超时退出后记录终态；暂时不能持久化的终态放在有界内存缓存，恢复后由客户端查询/Provider 状态得到一致结果。提交阶段结果未知时报告 STORAGE，不能假称取消成功或提交成功。实际子进程回收仍由数据 runner 验证。

这些修正需要新增 Core/宿主回归测试。Rust 编译状态不变；不安装 MSVC/SDK，也不修改前端或数据 agent 的实现。

`apps/desktop/src-tauri/src/composition.rs::bootstrap(app_data_dir, executable_dir)` 已根据 data.md 的公开入口注入真实 SqliteRepository、ProcessRunner、ClaudeCodeAdapter 和 SystemClock。release 只找安装目录的固定 sidecar 文件名；debug 可使用受锁校验的项目 binaries。缺失/不匹配的 executable 明确拒绝启动，不回退为 Mock 或 MemoryRepository。

组装使用数据 agent 公开的 SQLite/Runner/Claude Code 构造入口，传入 app data 路径、已验证的固定 executable、持久 dataset/device 身份和 SourceConfig，没有把 Tauri AppHandle 传入 Core。已读取 TauriUsageClient 并核对 `{ request }`/ApiError/API major 协商与宿主匹配；连续三次扫描/重启测试入口已增加，实际页面→IPC→Core→SQLite→Claude Code 全链路仍待 native 执行。

现已读取 frontend.md / data.md 的交付。前端直接消费冻结 API，未增加依赖；已复验 32 项测试、类型和 lint，沿用前端的生产构建与浏览器验收记录。数据增量依赖（sha2、runtime tempfile、windows-sys、libc）已纳入主 agent 的全局 Cargo.lock。未改两方的实现文件或交付记录。

主 agent 增量：

- Profile v1 使用独立宿主文件模型，UUID 原子 create-if-absent 并保留身份；默认不启用扫描。已有 DB 但身份文件丢失时拒绝新身份。当时来源开关仅由开发者编辑 profile；本次已按下方 API 1.1 扩展接入设置页面，真实原生持久化尚未验收。
- 第一注册的 Tauri single-instance 插件防止多个进程重复启动同一 Provider，只唤回已有窗口；不转发或记录第二进程参数/工作目录。参照 [官方单实例要求](https://v2.tauri.app/plugin/single-instance/)。
- Core 保留 queued.startedAt，拒绝复用终态 jobId。相同 UTC 时间/不透明 ID 不决定因果顺序；当前服务用观测顺序保持最新状态，冷启动完全相同时间采用保守错误状态。宿主 recovery/异常终态同步到此规则。
- native composition 测试覆盖构造真实 adapter/repo→Runtime→Core→DTO、连续三次采集合成日志保持 515 token、reasoning 未知和重开后 revision/identity。它不冒充已跑的真实 IPC，也尚未执行通过。
- 生成了 ccusage 的完整 MIT 许可文本并纳入 Tauri resources；由锁定 npm tarball SRI 校验后提取。其他依赖完整 notices 库存仍待完成。
- CI 两平台增加已锁 sidecar 准备、CLI fixture、Rust runner native 与宿主 composition ignored 测试；打包后验证实际 .app/Windows staging 中的 sidecar，不用下载目录替代包内证据。Windows 安装仍需实际 NSIS 烟测。
- `verify-bundle` 沿用数据脚本的原始 hash 严格校验。macOS ad-hoc/正式签名若改变字节会阻断包内校验/运行，可信签名后 manifest 尚未实现；没有弱化原始校验。macOS 全部状态保持未验证。

所有本地文件未提交，没有 Git 初始化、远程仓库、推送或发布。下一验收需要具备编译工具的 Windows/macOS ARM64 环境；用户此前明确不安装本机 MSVC/SDK。Settings 原生持久化验收、签名后 sidecar 身份、完整 notices、实际安装器大小/最低系统/安装烟测保持待办。

## Settings 扩展影响（先记录，后实施）

API 升为 **1.1.0**，major=1 保持查询/扫描/导出 wire DTO 不变。新增 getSettings / updateSettings / chooseProviderDirectory 三个 commands 和 DTO；UsageClient 使用可选扩展方法保证已有 1.0 客户端实现继续可构建。新能力为 settings-read/settings-write/source-directory-selection。

Provider 目录由宿主原生选择器选择并按现有 Adapter 规则校验，UI 只传不透明 directoryRef，不接收任意绝对路径。更新采用 expectedRevision，冲突要求重新读取；只修改 timezone/Provider enabled/目录引用，deviceId/datasetId 不出现在更新请求。扫描中拒绝修改配置，关闭扫描不清空历史。目录迁移沿用现有 dataset 身份，独立新数据集/重建仍不在此设置入口实现。时区改变只影响下一次查询/采集，不重新分桶现有 Daily。

宿主 profile v1 增加有默认值的设置 revision 与目录引用，可读已有 profile；Windows 原子替换使用 MoveFileExW，Unix 使用 rename + directory sync。前端/数据均已完成交付，后续集成只针对新增设置 transport、Mock 和 Settings 页面，不重写统计或修改数据实现；记录增量并重新验证原有测试。

本次已完成上述增量：Tauri/Mock transport 和 Settings 页面实际接入，目录取消保留当前设置，revision 冲突要求重读，保存后统一失效查询缓存并应用持久 timezone，来源开关只控制采集、不删除历史。Mock 设置明确仅当前页面会话有效、目录选择不访问磁盘。原 1.0 服务缺能力时不发送未知命令。

浏览器（localhost 独立 Mock）实际验证：关闭 Claude→保存→Providers 扫描按钮禁用；回到 Settings→选择合成目录→恢复启用→保存，显示“未修改磁盘文件”。控制台 warn/error 为空；新增布局截图 `artifacts/settings-api-1.1.jpg` 已检查。真实 Rust 原生选择器/MoveFileExW 与 IPC 仍未运行，不能把浏览器 Mock 验收当成后端持久化证据。

源码新增 profile 兼容默认、设置版本/任意引用拒绝、扫描中写入拒绝等 Rust 回归测试；未执行通过。剩余工作、阻塞与出口已整理到 REMAINING_WORK.md。

## v0.0.1 公开源码准备

用户于 2026-10-04 明确授权发布当前项目到 GitHub public repo，版本为 v0.0.1；下一步再实施免费标准 runner 的 Tauri 自动编译打包。项目此前没有 Git 仓库，本次将以项目目录为仓库根（不包括父目录缓存），发布目标为 upuphero/agent-usage-dashboard。应用 manifest/Cargo workspace/lock/UI 演示版本统一为 0.0.1，API 保持 1.1.0。

为按顺序先发布源码，现有 ci.yml 改为 workflow_dispatch，package.yml 继续只手动触发；不在初次 push 或 tag 上自动运行未验收的矩阵。v0.0.1 作为 source preview prerelease，没有安装器；Rust、实际 IPC、macOS/安装/签名后 sidecar 继续未验证。下一步需在远程 runner 完成验证后再启用自动流水线。

公开准备排除 node_modules/target/dist、原生二进制、临时研究、数据库/profile、日志、截图和凭据文件；前端交付记录的个人用户目录已替换为 APPDATA 通用路径。MIT 许可与 ccusage notices 随源码发布；完整 transitive notices 仍是分发门槛。未提交其他任务未完成的研究或生成物。

## 免费 GitHub Actions 原生构建集成（2026-10-04）

用户授权继续完善并运行公开仓库的免费编译/打包流水线。只使用 ubuntu-24.04、windows-2022/x86_64 和 macos-15/ARM64 标准 runner，私有仓库条件跳过；无付费 runner、cache 或签名凭据。main push、PR、v* tag 和手动触发统一先执行可复用验证，再构建两平台；只有两平台安装包、包内 sidecar fixture 和 native tests 都成功才汇总资产。仅保留安装器和证据一天，不自动发布或修改现有 v0.0.1 source-preview Release/tag。

本次影响先记录：Rust API 1.1.0 / UsageSource / UsageRepository / DTO 不变。Tauri 明确 custom-protocol feature，生产嵌入前端资源；构建传 --locked。Windows 使用临时 runner 上真实 NSIS 静默安装，macOS 从实际 DMG 复制 app；安装后校验固定 SHA/架构并执行 fixture，后续 native 集成测试使用安装位置的 sidecar。真实 UI、Gatekeeper/SmartScreen、最低 OS 和完整 transitive notices 尚待验收。

首轮真实 Rust 编译暴露数据交付中 rusqlite 0.37 的旧 DatabaseName API：官方 v0.37.0 backup.rs 使用 MAIN_DB。主 agent 对已完成交付仅作此兼容性替换，保持 SQLite backup/迁移语义和依赖锁定不变。原始 data.md/frontend.md 不改。签名改变 sidecar 的问题仍坚持原始锁校验，等待实际打包结果；不跳过校验。

macOS 打包增量影响（实施前记录）：已核对锁定 Tauri CLI v2.8.4 官方 bundler 源码，externalBin 会被重新 codesign，破坏运行期坚持的原始 SHA。采用官方 macOS.files 将原始锁定 sidecar 逐字节放到 Contents/MacOS/ccusage，避免重签来源程序；主程序/.app 仍 ad-hoc 签名。构建先验证原始 SHA、架构、fixture 和 sidecar 现有代码签名；从实际 DMG 安装后再验证原始 SHA、嵌套签名结构和 fixture。没有新的运行时 hash 预期、没有关闭校验、没有签名凭据或 Developer ID 信任声明。平台配置和构建测试由主 agent 管理，数据 runner 不改。

首次原生验证结果：8d735a1 上 Windows NSIS、macOS ARM64 DMG 已构建；两平台各 48 项默认 Rust tests 和 3 项显式 native tests 实际通过（受控 runner、SQLite、DTO、三次扫描/重启、未知 reasoning）。宿主严格 clippy 阻断两平台资产，诊断为 composition 两个复杂 tuple 返回类型与 Settings 一个多余 unit expression。主 agent 只提取宿主内类型别名、移除冗余表达式；API/领域/存储语义不变，不降低 lint 门槛。下一轮将从实际安装器验证 sidecar，再执行安装位置 native tests 和 clippy。纯 Markdown/许可/ignore 改动不重复触发完整编译，tag/手动仍可构建。

## 免费原生 CI 验收已完成

[run 37251178175](https://github.com/upuphero/agent-usage-dashboard/actions/runs/37251178175) / commit fae5666d3c74688fa38fefca6a90ee703bf1974f 的 5 个 jobs 全部 success。前端 38、Node/SQL 16、各平台 51 项 Rust/native tests 和严格 clippy 通过；从实际 NSIS 安装 / DMG 复制 app 后，原始锁 SHA/架构/Unicode 空格路径/fixture 验证和重复 3 项 native tests 通过。Mac ad-hoc 与上游 sidecar 原始签名共存，codesign --verify --deep --strict 成功，固定 sidecar bytes/hash 没有改变。

Windows NSIS 4146040 bytes（3.95 MiB）；Mac ARM64 DMG 5167856 bytes（4.93 MiB）。两平台集中 artifact 已生成，含相对路径 SHA256SUMS、size-report、bundle-manifest、build-info 和 ccusage notices；保留一天，本地下载后再次实际校验两安装器 hash。完整尺寸、hash、image、签名与验证边界见 ../ci-validation/2026-10-04.md。安装器二进制只在忽略的 artifacts 目录，不提交。

接口基线已就绪，API 1.1.0 / 各端口不变；前端、数据已完成交付并集成。纯文档最终状态更新不触发重复完整编译。仍待真实 UI/IPC、最低 OS/干净机器/升级、完整 transitive notices、正式签名和用户日志验收。本机 MSVC/SDK 仍按用户要求不安装，远程成功不冒充本机编译成功。v0.0.1 旧 source-preview tag/Release 不变；不自动发布二进制或配置凭据。

## Codex / Antigravity 与 Windows 免安装增量（实施前影响）

用户在 Windows 当前只使用 Codex 和 Antigravity，要求先统计两者；此前只接入 Claude Code 导致无数据。两份原任务交付已完成，主 agent 负责此次必要跨层集成，不启动 agent，也不改原始 data.md/frontend.md。

保留 Core/SQLite/Tauri/锁定 ccusage 架构及 API 1.1.0 端口/DTO。新增 ccusage.codex / ccusage.antigravity 产品级 Adapter；固定 source 专属 CLI 参数和环境，保留原 Claude runner 入口。Codex report inputTokens 已排除 cache，reasoning 是 output 子集；Antigravity report outputTokens 是可见输出，totalTokens 包含推理，需在 Adapter 恢复完整 output，并保守处理未提供的模型推理拆分。前端不重算这些口径。

Settings/profile 扩展为多个 Provider，目录引用绑定 provider，迁移保持已有 device/Claude dataset 身份，为新增来源创建并持久化独立 dataset；不默认扫描。首次使用提供明确“启用并扫描”入口、来源不可读诊断、近期范围。只读提取本机用量，不读认证文件，不把聊天正文/源数据库提交或上传；fixtures 全部合成。

Windows 增加免安装 ZIP：主 exe、锁定 ccusage.exe、必要 resources/notices 同目录；CI 验证解压包架构/hash/fixture/native tests。数据仍保存在应用用户目录，便于免安装与安装版共享身份/历史；ZIP 不承诺完全不依赖系统 WebView2。两平台正式签名/最低系统/真实 UI 仍按实际证据报告，不扩大 Core 耦合。此迭代使用应用 0.0.2 区别于旧 v0.0.1 测试包，不修改旧 tag/Release。

0.0.2 增量已验证并集成 main：run 37258630412 / 0b4cab88b0601e02908ce64aa9f42e5277e122ce，五 jobs success；前端 38/Node SQL 16/Linux Rust 36/各平台默认 53 + native 4 + 安装或便携目录再跑 4 tests、strict clippy 均通过。Windows portable ZIP 5784690 bytes（5.52 MiB）、NSIS 4206115 bytes、ARM64 DMG 5244007 bytes，本地再次验证三份 SHA。

已在本机执行便携包的无界面诊断（不改旧 GUI/profile/cache），新 Rust 受控 runner/冻结元数据/Adapter/Core/SQLite/DTO 链路读取真实 Codex 和 Antigravity，两扫描 succeeded。私人统计只放忽略的 artifacts/private-usage；不提交、不上传，不读取认证文件。不是用 Mock 或独立 CLI 结果冒充新版后端。未知字段、Antigravity 多模型推理拆分、.pb 未支持等限制保留 warning/nullable；正式 GUI全点击和真实 Mac 数据仍未验收。

profile v1 → v2 原子迁移已加测试：保留既有 device/Claude dataset，新增来源独立稳定 UUID，拒绝新版 profile 被旧二进制读取。Settings 支持多 Provider，目录引用绑定来源。Default range 最近30天，关闭来源有“启用并扫描”入口。本次旧 v0.0.1 tag/Release 保持不变，未发布正式安装器。详细证据与操作见 ../ci-validation/0.0.2-codex-antigravity.md。

## 0.0.3 UI 可读性增量（实施前记录）

用户要求每日完整数字只在悬停显示、筛选收为图标展开、增加曲线图并将趋势放概览最前、精确数字下补中文量级。1,855,654,157 正确量级为18亿5565万（约），不是1亿8千万。只改展示/交互：使用API原始字符串的BigInt格式，图形比例计算不改变业务值；未知值/缺失时间区间不填零，不改变统计、价格、Provider/SQLite或API1.1.0。

曲线默认、保留柱状切换，完整数值tooltip支持鼠标/键盘/触摸，轴标签稀疏显示。筛选按钮默认折叠、显示当前查询摘要，保留原有查询参数与能力限制，支持Escape/外部点击关闭。中文量级只在中文UI显示，精确值始终保留。前端任务已交付，主agent做用户指定的目标修改，不启动agent、不改原始frontend.md/data.md。应用增量为0.0.3；继续免费标准runner输出Windows免安装/NSIS和ARM64 DMG。

0.0.3 图形优先 UI 已完成：实际浏览器验证30个大数值点、完整数值提示与方向键、曲线/柱状切换、紧凑筛选/外部关闭/Escape/能力联动、中文量级；1180和800桌面窗口无横向溢出、console无warn/error。41前端tests通过；新图形不改变API1.1.0或统计值。免费 run37261635910 / 3e6cb88b2e75fe2e99aff07c5debd0ce3fa65066 五jobs全通过，各平台native/安装或解压/lint/原始sidecar门槛保持通过。Windows ZIP5778704 bytes/NSIS4203562 bytes/Mac DMG5244795 bytes已下载重新核对hash；证据见../ci-validation/0.0.3-chart-readability.md。截图/合成压力预览仅本地忽略目录，未上传，也不把浏览器Mock验收当真实Tauri点击验收。数据/历史/profile2保持不变，不改旧tag或发布凭据。

## 0.0.4 零用量日期与模型排序（实施前记录）

用户指出无使用日期应为0、曲线不能因API省略日期而断开；0.0.3把省略桶与未知字段一同断开是主agent的问题。本次由Core补齐查询周期中完整标准Daily覆盖证明没有记录的桶，值为derived zero；未采集、明确null或不完整覆盖仍不伪造零。UI只画API的完整时间序列，不自己补业务数字，保证统计总计不因补展示桶而增加。

模型分布提供token和API估算成本两种精确降序排序；已知零用量折叠在底部。截图中null token但有成本/细分用量的模型并非未使用，按当前排序字段不足单独放末尾折叠，避免伪装零。费用按API已有金额排序，不新建价格规则；BigInt/定点字符串比较保留精度。API1.1.0和端口不变，应用0.0.4。两方已完成交付，主agent做必要修正，不启动新agent、不改原始frontend.md/data.md。

0.0.4 已完成：Core 日/ISO周/月补桶、明确未知保持未知、总计和记录计数不变；Overview 最大10,000天限制补桶内存。实际合成浏览器验证零值连续曲线、完整提示、两种降序、折叠展开/收起和无console错误。免费 run37265070751 / 25e115a3634aa03d0082aa3999f1734a36d76927 五jobs success；44前端/16Node SQL/39Linux Rust、各平台56默认 + 4native + 安装或解压重复4native tests、strict lint、固定sidecar原始hash保持通过。Windows ZIP5784720 bytes/NSIS4208640 bytes/Mac DMG5250039 bytes已下载逐一重验SHA；详细证据见../ci-validation/0.0.4-zero-periods-model-ranking.md。设置/缓存仍沿用同一用户目录，无需重建数据集；截图/日志仅忽略目录，真实GUI与最低OS/正式签名等边界不扩大。
