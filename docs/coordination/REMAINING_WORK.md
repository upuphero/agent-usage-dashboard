# 剩余工作与验收顺序

更新：2026-10-04。应用 0.0.2 / API 1.1.0；仅 Windows x64、macOS Apple Silicon ARM64。

新增 Codex / Antigravity 与 Windows portable ZIP 已完成：[run 37258630412](https://github.com/upuphero/agent-usage-dashboard/actions/runs/37258630412) 全部通过；本机便携包的新 Rust 后端已只读扫描真实两来源并 succeeded，私人结果未上传。profile v2 迁移保留旧身份、每来源独立 UUID，多 Provider 设置与“启用并扫描”、最近30天范围已落地。Antigravity .pb 未纳入、模型输出拆分不足时保持 unknown；完整真实 GUI 点击流程和真实 Mac 数据仍待验收。[0.0.2 证据与操作](../ci-validation/0.0.2-codex-antigravity.md)

| 优先级 | 工作 | 当前状态 / 具体出口 |
| --- | --- | --- |
| P0 | Settings 读写与目录选择 | 代码、38 项前端 tests 和 Mock 交互通过；两平台 Settings revision/身份保持/原子持久化 Rust tests 已通过。真实原生选择器与完整 UI/IPC 操作仍未验收。 |
| P0 | 真实 UI/IPC 闭环 | 本机 MSVC/SDK 不安装；远程两平台 Rust tests、clippy、native backend 闭环已通过。仍需实际启动 Tauri，从 UI 启用→选择目录→扫描→查缓存→取消→退出→重启→导出，确认 UI 与真实 IPC 配合。native DTO 测试不代替 UI 验收。 |
| P0 | 正式签名与分发 | 当前 Windows 未签名、Mac ad-hoc，无 Developer ID/公证。macOS.files 原样保留上游 sidecar 的签名/原始 SHA，真实 DMG 安装后 hash、嵌套签名结构及运行已经通过。未来若正式重签 sidecar，需另行设计可信身份，不绕过原锁；凭据和正式发布另行确认。 |
| P1 | Windows 剩余安装场景 | NSIS 在 windows-2022 实际安装、中文/空格路径、主程序/sidecar 架构和原始 hash、安装后 fixture/native tests 已通过。真实 UI、干净机器无额外 runtime、WebView2 缺失、升级 DB 保留和最低 OS 尚未验证。 |
| P1 | macOS 剩余安装场景 | macos-15 ARM64 已实际执行 CLI/runner、构建 app/DMG、挂载复制、权限/嵌套签名/原始 hash 和安装后 native tests。真实 WKWebView UI、下载后的 Gatekeeper、公证、最低 OS 和升级仍待验证；Intel/universal 不在范围。 |
| P1 | 完整许可与构建材料 | ccusage MIT 已随资源打包；commit/工具链/runner image/版本/主程序和 sidecar/安装器大小与 SHA 已在集中 artifact 和持久验收记录。完整 Rust/npm/native transitive notices 库存仍待补。 |
| 完成 | 免费 CI 和集中资产门槛 | run 37251178175 五个 jobs 全部通过；两平台各 51 tests + 安装后重复 3 tests、strict clippy、实际安装校验及集中 SHA256SUMS 已完成。本地再次校验下载的两份安装器 hash。标准 public runner、无 cache、artifact 一天，不自动发布 Release。 |
| P2 | 用户交互与诊断扩展 | 当前设置已支持基础项；数据目录备份/清除、采集版本元数据、精细扫描订阅仍未开放。分别定义接口和验证，不混入统计实现。 |

用户已于 2026-10-04 授权创建 GitHub public repo、首发 v0.0.1 源码预览和继续免费 CI 打包。当前有 main commit fae5666d 的 Actions 测试安装器，旧 tag/Release 未变。正式签名凭据和生产操作仍另行确认。未通过项目保持未验证；本文件不代表桌面软件已达到完整 Definition of Done。[持久验收记录](../ci-validation/2026-10-04.md)

后续原生构建可使用已通过的免费 Actions；不再次申请安装本机系统组件，不启动新的 agent，不改两份原始交付记录。数据实现仅有已记录且通过两平台 tests 的 rusqlite 0.37 MAIN_DB API 兼容修复。
