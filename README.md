# Agent Usage Dashboard

**v0.0.1：首次公开源码预览。** 当前没有 Windows/macOS 安装器；完整原生编译、实际 IPC 与安装验收仍待完成。[版本说明](docs/releases/v0.0.1.md)

Windows x64 / macOS Apple Silicon ARM64 的本地 AI 用量桌面应用。Tauri 2 + React/TypeScript + Rust Core + SQLite + 锁定 ccusage sidecar；不开发 Intel Mac。

开发状态：API 1.1.0、Dashboard/Mock/Tauri transport 和 Claude Code 数据代码已交付。宿主已改为真实 SQLite + 固定 ccusage runner + ClaudeCodeAdapter 的依赖组装，持久化设备/数据集 UUID，加入三次扫描/重启的 native 测试入口。前端 38 项和 Node 脚本/SQL 14 项检查通过，Windows 原生 CLI fixture 通过；完整 Rust/Tauri/SQLite runner 链路在本机因缺 MSVC/SDK 尚未验收，macOS 未验证。不能视为可发布软件。

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

桌面首次启动在 Tauri 应用数据目录创建 `profile.json` 和 `usage.db`，扫描默认关闭。Settings 页面已接入读取/保存统计时区、来源开关和原生目录选择；UI 只传目录引用，版本冲突需重新读取，扫描中不能改配置。保存保持 deviceId/claudeDatasetId；设置文件丢失而 DB 已存在时拒绝生成新 ID，防止同一数据集重复计数。时区改变需重扫，旧 Daily 不会被伪造重分桶。Mock 设置仅保留当前页面会话且不访问磁盘；真实保存/原生选择仍待 Rust/IPC 验收。

native 合成测试只读测试 fixture，不访问真实日志。在有编译工具的终端准备资源、设置 `CCUSAGE_TEST_BINARY` 为其绝对路径，再执行：

```text
cargo test -p usage-adapters --test native_pipeline --locked -- --ignored
cargo test -p usage-desktop --locked -- --ignored
```

包内检查入口 `scripts/verify-bundle.mjs` 验证主程序架构、具体包位置的 sidecar 哈希并运行同一套合成 fixture。[GitHub Actions](docs/GITHUB_ACTIONS.md) 使用 public repo 的免费标准 Windows x64/macOS ARM64 runner，main push、PR、tag 或手动触发；先验证，再构建并从真实 NSIS/DMG 验证安装程序。两平台都成功才提供集中 artifact 和 SHA256SUMS，保留一天，不自动发布 Release。当前首次原生流水线正在验收，完整 notices、真实 UI/IPC、最低 OS 和正式签名仍待完成。

协作入口：`docs/coordination/CONTRACT_BASELINE.md`、`architecture.md`；前端维护 `frontend.md`，数据维护 `data.md`。API 请求范围 `[start,end)`、token 字符串、未知值 null。Overview 只统计标准 Daily，Sessions 展示会话累计用量，禁止两个报表相加。费用始终是 API 等价估算成本。
