# Agent Usage Dashboard

**0.0.2：Windows 免安装 + Codex / Antigravity。** 两平台免费 Actions 已通过，Windows 提供解压即用 ZIP 和 NSIS，Mac 提供 ARM64 DMG。[最新构建与使用](docs/ci-validation/0.0.2-codex-antigravity.md) · [流水线](docs/GITHUB_ACTIONS.md) · [旧源码首发](docs/releases/v0.0.1.md)

Windows x64 / macOS Apple Silicon ARM64 的本地 AI 用量桌面应用。Tauri 2 + React/TypeScript + Rust Core + SQLite + 锁定 ccusage sidecar；不开发 Intel Mac。

开发状态：API 1.1.0 保持兼容，已接入 Claude Code、Codex、Antigravity。CI 已通过前端 38、Node/SQL 16、两平台各 53 个默认及 4 个 native Rust tests、严格 clippy、安装/免安装解压后再次执行的 4 个 native tests。Windows ZIP 5.52 MiB / NSIS 4.01 MiB / Mac DMG 5.00 MiB。新后端已在本机只读扫描真实 Codex/Antigravity，两个来源 succeeded，私人用量未上传。真实 GUI 全流程、最低 OS、完整依赖 notices 和正式签名仍待验收；本机 MSVC/SDK 未安装。

Windows 免安装：先退出旧版，解压整个 portable.zip，双击 usage-desktop.exe，并保留同目录 ccusage.exe。进入数据来源分别点击 Codex/Antigravity 的“启用并扫描”；默认目录找不到时在设置选择目录。默认显示最近 30 天；未扫描与未知字段显示不可用，不冒充零。需要系统已有 WebView2；配置/缓存仍保存到用户应用数据目录，与安装版共享历史。

```text
pnpm install --frozen-lockfile
pnpm contracts:check
pnpm boundaries:check
pnpm version:check
pnpm scripts:test
node --test crates/usage-adapters/tests/*.test.mjs
pnpm dev
pnpm lint
pnpm typecheck
pnpm test
pnpm build
cargo test -p usage-core -p usage-contracts -p usage-adapters --locked
```

工具版本由 `.node-version`、`packageManager`、`rust-toolchain.toml` 与两个全局 lockfile 固定。Windows Rust/Tauri 需要已有 MSVC C++ Build Tools 与 Windows SDK；不自动安装系统组件。

```text
pnpm desktop:build --local --target=x86_64-pc-windows-msvc
pnpm desktop:build --target=x86_64-pc-windows-msvc
pnpm desktop:build --target=aarch64-apple-darwin
```

准备锁定资源：

```text
node scripts/prepare-ccusage.mjs --target x86_64-pc-windows-msvc
node scripts/verify-sidecar.mjs --target x86_64-pc-windows-msvc
node scripts/prepare-notices.mjs --check
```

macOS 在 ARM64 主机使用对应 target。build 脚本拒绝跨目标、Intel Mac 与 universal。Windows 配置 NSIS + WebView2 下载引导；macOS 配置 ARM64 DMG + ad-hoc 开发签名，13.0 是暂定 deployment target，最低兼容系统仍待实测。macOS 使用官方文件映射保留已校验 sidecar 的上游签名与原始字节，运行期继续坚持锁中原始 SHA。

桌面首次启动在 Tauri 应用数据目录创建 `profile.json` 和 `usage.db`，扫描默认关闭。Settings 支持三个 Provider，目录引用绑定 Provider，版本冲突需重读，扫描中不能改配置。profile v1 → v2 保留 device/Claude dataset，为新增来源各自持久化 UUID；旧二进制不能读取新版 profile，需先退出旧版本。配置丢失而 DB 已存在时拒绝新身份。时区改变需重扫，旧 Daily 不会伪造重分桶。Mock 设置仅当前页面会话有效、不访问磁盘。

Codex 读取用户 .codex/sessions 与 archived_sessions；Antigravity 读取已知本机 conversation .db，当前锁定版本不支持 .pb，界面 warning 标明未纳入范围。冻结输入只含用量白名单元数据，不复制认证/用户配置/正文或整个数据库。源统计和价格使用固定 ccusage，公共聚合/快照替换仍只在 Core。费用为 API 等价估算，未记录的 Codex 档位按标准估算；未提供的模型拆分显示 unavailable。

native 合成测试只读测试 fixture，不访问真实日志。在有编译工具的终端准备资源、设置 `CCUSAGE_TEST_BINARY` 为其绝对路径，再执行：

```text
cargo test -p usage-adapters --test native_pipeline --locked -- --ignored
cargo test -p usage-desktop --locked -- --ignored
```

包内检查入口 `scripts/verify-bundle.mjs` 验证主程序架构、具体安装位置的 sidecar 哈希并运行同一套合成 fixture。[GitHub Actions](docs/GITHUB_ACTIONS.md) 使用 public repo 的免费标准 Windows x64/macOS ARM64 runner，代码/config 的 main push、PR、tag 或手动触发；先验证，再构建并从真实 NSIS/DMG 检查安装程序。两平台都成功才提供集中 artifact 和 SHA256SUMS，保留一天，不自动发布 Release。Windows 未签名，Mac 为 ad-hoc 签名，无 Developer ID/公证。

协作入口：`docs/coordination/CONTRACT_BASELINE.md`、`architecture.md`；前端维护 `frontend.md`，数据维护 `data.md`。API 请求范围 `[start,end)`、token 字符串、未知值 null。Overview 只统计标准 Daily，Sessions 展示会话累计用量，禁止两个报表相加。费用始终是 API 等价估算成本。
