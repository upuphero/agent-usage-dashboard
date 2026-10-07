# 数据 agent 交付记录

## 当前数据接入状态（2026-10-07）

应用 **0.0.7**，锁定 ccusage **20.0.26**，Windows x64 / macOS ARM64 的固定 SRI/原始二进制 SHA 与安装后执行均已验证。[最终证据](../ci-validation/auto-full-scan-v1.md) · [当前待办](REMAINING_WORK.md)

- 已接入 Claude Code、Codex sessions/archived_sessions、Antigravity 支持的 conversation .db；主 conversation .pb 仍未支持。附属 metadata 的 protobuf 过滤不等于主 .pb 采集实现。
- 原生变化检查/监听复用 Adapter 目录解析和相同输入边界；JSONL 文件集合/身份/大小/mtime、DB/WAL、内容/不确定事件提示支持自动完整扫描。监听注册/过滤统一 canonical roots，Unix 别名回归在两平台通过。
- Core 定义 inspect/watch，Adapter 执行 IO；只在已启用范围工作。指纹限定 provider/dataset/roots/timezone/collector/normalization，属于内存成功基线，不是解析游标或事件级账本。
- 完整 Daily/Session、SQLite 原子替换、修正/失败/取消/不完整覆盖保留历史、缺失与零分开、固定离线价格和 UUID 身份规则保留；三个 supportsIncrementalCollection 与 supportsQuota 仍为 false。
- 当前 17 项 Adapter lib tests、其余存储/进程/统计/宿主回归及两平台 native fixture 已通过；完整总数与 installed-sidecar 证据见最终记录。macOS 不再处于“仅验证架构、未执行”的状态。

真正增量、用户数据维护/归档导入/多设备、.pb 与更多来源分别是 T7–T9/T13–T15。最低 OS、真实用户日志全矩阵与硬件/GUI 场景未完成，不把合成 fixture 当成这些验收。

## 首阶段历史交付（2026-10-04）

下文的单来源范围、Windows 本机执行/缺 link.exe、Mac 未执行和 13 项源码测试是当时事实，后续已扩展/验证；原始校验来源和业务口径仍保留，当前支持状态以上方摘要为准。

日期：2026-10-04。基线：`CONTRACT_BASELINE.md` / Core 已冻结接口 / API 1.0.0。范围仅 Claude Code。

**代码已实现；Windows 原生 CLI、Node schema/脚本测试和 Rust 格式检查已通过。Rust 编译、Rust 集成测试、受控 runner 的平台运行验收尚未完成。macOS ARM64 只有下载、完整性与架构证据，未执行。不能据此宣布完整桌面闭环已验收。**

只修改 `crates/usage-adapters`、其 migrations/tests、`tests/fixtures/claude-code`、两个授权 sidecar 脚本、`ccusage.lock.json` 和本记录。没有改 Core/API/前端/桌面组装/根构建配置/全局 lockfile；没有启动其他 agent、发布、推送或读取真实用户日志。研究文件在 crate 的 `.local-research/`，由 crate 自己的 `.gitignore` 排除。

## 固定发行物与来源

候选 `20.0.26` 已实际从 npm registry 核实，两个目标包均 HTTP 200。没有把开发文档或在线 main 的版本陈述当作下载事实。

| 目标 | 包 / 精确版本 | tarball 字节 | binary 字节 | 实际验证 |
| --- | --- | ---: | ---: | --- |
| `x86_64-pc-windows-msvc` | `@ccusage/ccusage-win32-x64@20.0.26` | 2221764 | 4222976 | npm SRI、提取后二进制 SHA-256、PE32+ AMD64、`--version`、Claude daily/session fixture |
| `aarch64-apple-darwin` | `@ccusage/ccusage-darwin-arm64@20.0.26` | 2093820 | 3446176 | npm SRI、提取后二进制 SHA-256、Mach-O ARM64；**未执行** |

Windows 二进制 SHA-256：`ba5311e2f982c93a6b94dde5ca5488755f0f881b9836075076d58765d75fd2ce`。

macOS ARM64 二进制 SHA-256：`6d816ab7e989d475f19b8178330435209b31b7714496f1fcbc9bdf6951082d03`。

完整 tarball URL、npm `dist.integrity`、包内路径、资源名、尺寸、运行状态均已提交至 `ccusage.lock.json`。SRI 来自 [Windows npm 版本元数据](https://registry.npmjs.org/@ccusage%2fccusage-win32-x64/20.0.26)、[ARM64 npm 版本元数据](https://registry.npmjs.org/@ccusage%2fccusage-darwin-arm64/20.0.26)。SHA-256 是对 SRI 已校验 tarball 的原生文件计算后固定提交的值；准备脚本只比较预期值，不现场生成新预期值。

主包和两个原生包的 npm provenance 都指向提交 `d9821088b98aa536c7a385aa1a4579d6fa02269b`，GitHub `v20.0.26` tag 也指向该提交。实际读取并检查了该提交的 [Claude loader](https://github.com/ccusage/ccusage/blob/d9821088b98aa536c7a385aa1a4579d6fa02269b/rust/adapters/claude/src/lib.rs)、[daily loader](https://github.com/ccusage/ccusage/blob/d9821088b98aa536c7a385aa1a4579d6fa02269b/rust/adapters/claude/src/daily.rs)、[路径发现](https://github.com/ccusage/ccusage/blob/d9821088b98aa536c7a385aa1a4579d6fa02269b/rust/adapters/claude/src/paths.rs)、[配置加载](https://github.com/ccusage/ccusage/blob/d9821088b98aa536c7a385aa1a4579d6fa02269b/rust/crates/ccusage-config/src/config.rs)、[离线价格](https://github.com/ccusage/ccusage/blob/d9821088b98aa536c7a385aa1a4579d6fa02269b/rust/crates/ccusage-core/src/pricing.rs)。校验和及 provenance 的内容核对不等于在此实现 npm 签名信任链验证。

许可证实际取自 `ccusage@20.0.26` 的 SRI 已验证 tarball `package/LICENSE`：MIT，`Copyright (c) 2025 ryoppippi`。原生 tarball 仅包含 binary 与 package.json；主 agent 应在最终 notices 中包含完整 MIT 文本，来源见 lock 的 `license`。未修改全局 notices 或发行包配置。

## 已验证命令与配置隔离

```text
ccusage --version
ccusage claude daily --json --offline --breakdown --mode calculate --order asc --timezone <IANA_ZONE> --config <owned-empty-json>
ccusage claude session --json --offline --breakdown --mode calculate --order asc --timezone <IANA_ZONE> --config <owned-empty-json>
```

固定 JSON 为 `{daily:[...],totals:{...}}` 与 `{sessions:[...],totals:{...}}`。`date` / `sessionId`、`modelsUsed`、`modelBreakdowns`、`firstActivity`、`lastActivity`、`missingPricing` 和 `totals.unpricedModels` 都已实际出现。完整最小 stdout 在 fixture 中。`type/data` 或统一多来源 `period/agent` 形状不属于此 adapter 的 schema，会拒绝。

每次子进程用私有临时 cwd 和显式 `{}` config；清空继承环境，设置唯一 `CLAUDE_CONFIG_DIR`、独立 HOME/USERPROFILE/XDG 路径、NO_COLOR，Windows 仅继承 SystemRoot/WINDIR。源码说明显式 `--config` 禁止配置 auto-discovery；实际测试也写入了会将日期限制到 2099 年且关闭离线的来源配置，输出仍保持 fixture 结果。CLI 不扫描其他产品，也不执行 npx/npm。

实际 Windows 测试使用新空缓存和不可连接的 HTTP(S) 代理；固定源码的离线分支使用内嵌价格并不调用在线加载。这是默认离线的证据，**没有做封包抓取或防火墙级网络验收**。

## 字段映射与精度

| 来源字段 | Core 字段 | 处理 |
| --- | --- | --- |
| `inputTokens` | `input_uncached` | exact，直接映射；不再扣 cache |
| `cacheReadTokens` | `cache_read` | exact，互斥输入桶 |
| `cacheCreationTokens` | `cache_write` | exact，互斥输入桶 |
| `outputTokens` | `output_total` | exact，保留来源完整输出；不另加 reasoning |
| 无独立 reasoning 报表字段 | `output_reasoning` | null/unavailable；能力列表不声明 reasoning |
| 父行 `totalTokens` | `total` | 保留直接报告值；与上游 totals 做 schema 一致性检查 |
| 模型行没有 total | `total` | 交给 Core `normalize_batch` 推导，derived |
| `totalCost` / 模型 `cost` | `amount_usd` | 数字字面值转 Decimal；estimated，USD TEXT 定点存储 |
| `missingPricing` / `unpricedModels` | `missing_models` | 未知模型零占位→null；混合行保留已估价部分并带缺价模型 |
| `date` | `RowDimension::Day` | 与 IANA 时区一起保存；不使用 session 活跃时间归日 |
| `sessionId` | `RowDimension::Session` | 使用来源真实 session 标识，不生成事件 ID |
| `firstActivity` / `lastActivity` | session 父行时间 | 来源确实输出才保存为 UTC；模型子行不复制整个 session 时间 |
| `projectPath`、正文、标题、其他原始字段 | 无 | 丢弃，不进入 Core/SQLite |

缓存语义由固定 loader 的四桶总和与 fixture 验证，并与 [Anthropic 缓存字段说明](https://platform.claude.com/docs/en/build-with-claude/prompt-caching#tracking-cache-performance)一致：`input_tokens` 是未缓存输入，cache 两桶不是它的子集。[Anthropic thinking 说明](https://platform.claude.com/docs/en/build-with-claude/extended-thinking)把 thinking 视为输出用量的组成部分；此固定 ccusage 报表未单独输出它，因此不从可见文本推算或填零。未验证新 API 的 thinking 明细在真实 Claude Code 日志中的出现情况。

价格模式固定 calculate/offline；`pricing_version=ccusage-20.0.26-embedded-calculate`，`pricing_as_of=null`。不实现第二套价格表，不声称估算是订阅支出。上游成本已经使用浮点运算，其浮点尾差仍属于估算；Decimal 只保证之后存储/传输不再次使用二进制浮点记账。

原始 JSONL 的缺失桶被上游默认补零。适配器先做限定目录的只读 audit；有缺失时保守地将该桶在整个 batch 中标为 null，并将不可信 total/cost 置 null，附 `SOURCE_METRICS_INCOMPLETE`。不会把缺失伪装为 exact zero。仅 nested cache_creation、advisor 等扩展结构尚无专门 fixture，不能宣称这些结构已稳定支持。

总计对象只做来源一致性校验，不产生另一条可求和行。Daily 与 Session 返回两个独立快照；父行 model_id=None，模型子行有 model_id。公共聚合、历史维度保护、过滤、缺失覆盖和去重均复用 Core，没有增加另一套业务统计算法。

## 实现与历史语义

- `ClaudeCodeAdapter` 实现冻结的 `UsageSource`，product/provider 分别为 `claude-code` / `ccusage.claude-code`。scope 固定 Standard；支持 day/model/session 与 input/output/cache/cost/total；无 events、quota、增量、日期×session。
- 配置来源是绝对 Claude config 根目录或其 projects 目录，默认只选 `~/.claude`。XDG 的另一套路径需显式配置，不隐式合并两个根。拒绝逗号路径，防止 ccusage 的逗号环境列表扩大采集范围；空格/中文已在 Windows CLI 测试。
- audit 只递归读取该 projects 下的 JSONL，不读取认证文件；不跟随内部 symlink，最多 32 层/100000 个文件/每行 4 MiB。流式解析仅留文件内容 hash 和缺失字段标志于内存。坏 JSONL 尾行、权限问题、已知字段不兼容或两次 audit 内容变化会失败。固定 ccusage 可能静默跳过坏行/读失败，因此不能只依赖 CLI 退出码。此版本 loader 使用紧凑的 `"usage":{` 标记；含 usage 的空格变体会被保守拒绝为 SchemaUnsupported，而不静默计为零。
- 两次 audit + 两个 CLI 命令是全量读取，不是增量；记录 collection_started_at/collected_at，warning=`REPORTS_READ_SEPARATELY`。活跃写入可能触发 CoverageIncomplete，需下一次重试；不宣称来源数据库级一致读取。audit hash 不作事件 ID 或存储去重键。
- `ProcessRunner` 固定已校验原生程序、参数和环境，默认每命令 30 秒、stdout+stderr 合计 16 MiB。并行排空两管道，stderr 不保留；取消/超时/限额后终止并 wait，future drop 使用 kill_on_drop、管道任务 abort 和进程树 guard。Windows 隐藏窗口并加入 KILL_ON_JOB_CLOSE Job Object；Unix 使用独立进程组。具体 runner 的取消、进程树回收测试源码已提供，但本机 Rust 未编译，**尚无这些实现已运行成功的证据**。
- `SqliteRepository` migration v1：STRICT 表、signed 64-bit INTEGER token、USD TEXT、外键、WAL、2 秒 busy timeout、查询/日期/session/scan 索引。snapshot/row identity 用结构化 JSON 编码为非空 TEXT 主键，不依赖 NULL UNIQUE 行为。
- 共享连接 Mutex + spawn_blocking 协调写入；独立连接由 SQLite IMMEDIATE transaction 协调。一个 batch 的全部快照在一个事务里替换同 key 行并增 revision；模型拆分删除旧行，绝不累加。dataset/provider/product/origin 所有权冲突拒绝且整批回滚。
- `load_snapshots` 用一个读事务绑定 metadata 和 rows 的 revision；统计交给 Core。`save_scan` jobId upsert、限定 provider，其他 scan 查询已实现。只存规范化白名单 metadata/metrics，未保存原始 stdout/stderr 或完整 CLI JSON。
- migration 用事务，已有非空库迁移前用 SQLite backup API 备份；更高 user_version 拒绝。migration checksum 规范化 CRLF，避免 Windows checkout 改行尾后无法重开。`backup_to` 使用 SQLite backup API 包含 WAL，拒绝覆盖现有文件。
- 失败 batch 不提交。正常扫描中维度消失/日志轮转的保留保护来自 Core run_scan，未另建 partial 合并或显式清库功能。repository 只按已验证 complete batch 提交，宿主必须走 Core 服务进行普通扫描。

## 主 agent 组装接口

公开入口（`usage_adapters`）：

```rust
ProcessRunner::new(executable: impl AsRef<Path>, limits: RunnerLimits) -> Result<ProcessRunner, CoreError>
RunnerLimits { timeout: Duration, output_bytes: usize } // Default: 30s / 16 MiB
ClaudeCodeAdapter::new(runner: ProcessRunner, source_dataset_id: String,
    origin_device_id: String, default_root: Option<PathBuf>) -> Result<ClaudeCodeAdapter, CoreError>
SqliteRepository::open(path: impl AsRef<Path>) -> Result<SqliteRepository, CoreError>
SqliteRepository::in_memory() -> Result<SqliteRepository, CoreError>
SqliteRepository::backup_to(destination: impl AsRef<Path>) -> Result<(), CoreError>
```

最小注入示例（宿主负责应用数据目录、生产 Clock、持久化 UUID 和配置；本任务未修改宿主）：

```rust
let runner = usage_adapters::ProcessRunner::new(sidecar_path, Default::default())?;
let source: Arc<dyn UsageSource> = Arc::new(usage_adapters::ClaudeCodeAdapter::new(
    runner, persisted_dataset_uuid, persisted_device_uuid, None)?);
let repository: Arc<dyn UsageRepository> = Arc::new(
    usage_adapters::SqliteRepository::open(app_data_dir.join("usage.db"))?);
let service = UsageService::new(repository, clock, vec![source]);
// SourceConfig { enabled: true, root_path: Some(selected_absolute_root) }
// 普通扫描一律 service.run_scan(...); 不从 command 直接 commit_batch。
```

ID 不从路径或 token 哈希生成，主 agent 应一次生成并持久化；改路径标签不自动改 dataset ID。`path_hint` 为 `<configured-claude-root>/projects`，不会向 UI 返回真实绝对路径。

**签名集成限制：** 当前 runtime runner 和 verify-sidecar 对锁中的原始二进制字节做哈希校验。后续 Authenticode/ad-hoc/Developer ID 签名若改变字节，不能继续使用原始 SHA 作为签名后校验值；主 agent 需在打包阶段与数据实现一起增加可信的签名后 identity/manifest 验证。当前没有验证签名后的 Tauri 包内运行，也没有修改 Tauri 配置。

## 依赖请求与接口问题

crate manifest 已声明：新增 `sha2=0.10.9`，将 `tempfile=3.23` 从 dev 移到 runtime，Windows `windows-sys=0.61` 的 Foundation/JobObjects/Threading/Security features，Unix `libc=0.2`。已有 rusqlite bundled+backup 保留。全局 lockfile 由主 agent统一维护；本任务未执行其生成/更新命令。收尾时共享 Cargo.lock 已包含这些依赖；协调工作仍在更新该文件，因此本记录不把某一时刻的全局 hash 当作最终锁定值。没有要求修改冻结 Core 端口或 API 契约。

第一次 `cargo test -p usage-adapters --locked --offline` 因 lock 需要更新而停止；收尾时共享 lock 已包含依赖，再执行同一命令阻塞于缺 `arrayvec v0.7.8` 缓存。另在 crate 排除目录内的私有复制 workspace 和私有 CARGO_HOME 下载锁定依赖并执行 `cargo test ... -p usage-adapters --locked`，实际编译阻塞是 **`linker link.exe not found`**（quote/proc-macro2/serde 等 build script），数据 crate 没有进入类型检查。本次未安装 MSVC/SDK，遵循基线用户决定。Rust 测试与 clippy 必须在具有依赖和 MSVC/SDK（或 macOS Xcode CLI）的环境补验。

该私有尝试的精确测试命令为 `cargo test --manifest-path crates/usage-adapters/.local-research/verify-workspace/Cargo.toml -p usage-adapters --locked`，其进程的 CARGO_HOME 指向同目录下 `cargo-cache`；没有改变用户全局环境变量。此私有镜像不是另一套正式端口，也不作为待集成源码。

## 验证命令与实际结果

在项目根执行。

```powershell
# 准备时从固定 npm URL 下载；也可 --archive <已缓存原始tgz> 完全离线准备。
node scripts/prepare-ccusage.mjs --target x86_64-pc-windows-msvc
node scripts/verify-sidecar.mjs --target x86_64-pc-windows-msvc
node --test crates/usage-adapters/tests/sidecar-scripts.test.mjs crates/usage-adapters/tests/migration.test.mjs
cargo fmt --all --check

# 主 agent更新全局 lockfile 且环境能编译之后：
cargo test -p usage-adapters --locked
cargo clippy -p usage-adapters --all-targets --locked -- -D warnings
$env:CCUSAGE_TEST_BINARY = (Resolve-Path apps/desktop/src-tauri/binaries/ccusage-x86_64-pc-windows-msvc.exe).Path
cargo test -p usage-adapters --test native_pipeline --locked -- --ignored
```

macOS ARM64 对应命令：

```sh
node scripts/prepare-ccusage.mjs --target aarch64-apple-darwin
node scripts/verify-sidecar.mjs --target aarch64-apple-darwin
CCUSAGE_TEST_BINARY="$PWD/apps/desktop/src-tauri/binaries/ccusage-aarch64-apple-darwin" cargo test -p usage-adapters --test native_pipeline --locked -- --ignored
```

跨主机只验证架构时使用 `verify-sidecar ... --architecture-only`，不会伪装成 runtime 验证。Intel Mac / Linux sidecar 目标直接拒绝。prepare 是纯 Node 标准库：白名单目标、限额下载、SRI、内存 tar 解析（拒绝越界路径/链接/重复条目/错误 checksum）、二进制哈希/架构、原子临时文件重命名、Mac chmod 0755；没有安装 Node/Bun 到用户应用。

| 验证 | 本次结果 |
| --- | --- |
| npm 元数据、主包/两 native tarball SRI、二进制 SHA-256、两架构 | 通过；校验值已锁 |
| Windows native version + daily/session fixture | 通过；Node 24.19.0 / Windows x64 `os.release=10.0.26200` |
| 默认离线 flags、空缓存、显式空 config 覆盖恶意 auto-config | 通过 CLI smoke；未抓包 |
| Windows repeat/correction/缺价/cache/跨午夜/时区/DST/跨年/可读空来源/空格中文路径 | verify-sidecar 通过 |
| Node 归档校验与目标拒绝 | 3 tests 通过 |
| Node 实际 SQLite migration STRICT/64-bit/NULL/主键/外键/索引/回滚 | 3 tests 通过；只证明 SQL schema，不冒充 Rust repository 运行 |
| cargo fmt --all --check | 通过 |
| cargo metadata --no-deps --locked --offline | 通过；manifest/依赖方向可读取，不代表类型检查 |
| Rust 测试/类型检查/clippy | **未通过验收**：私有验证已下载依赖，实际失败为 `link.exe` 缺失；根 workspace 离线缓存仍不全 |
| macOS ARM64 执行、安装包内 sidecar、最低 OS、干净机器、签名 | **未验证** |
| 真实用户日志 | **未验证，未读取** |

已提供 13 项默认 Rust 测试源码，覆盖映射/精度/隐私、合成 batch→SQLite→Core Overview/Sessions、三次重复扫描、修正与旧模型删除、失败/轮转保留历史与 stale、跨快照原子回滚、64-bit 上限、migration 回滚/备份/较新 schema、独立连接协调、DB 锁失败、scan upsert 与备份恢复；另 2 项 ignored native 集成测试覆盖真实 runner 的 fixture 链路和取消/超时/限额。它们尚未执行成功，不作为本次绿灯证据。

下一步由主 agent 核对统一 lock，完成编译与上面的 native tests，再做桌面组装；macOS 需在 ARM64 主机执行同一套验证。此交付不扩展 Codex、ChatGPT Web、DeepSeek 或其他来源。
