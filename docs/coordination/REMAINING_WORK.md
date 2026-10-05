# 剩余工作与验收顺序

更新：2026-10-04。应用 0.0.1 / API 1.1.0；仅 Windows x64、macOS Apple Silicon ARM64。

| 优先级 | 工作 | 当前状态 / 具体出口 |
| --- | --- | --- |
| P0 | Settings 读写与目录选择 | 本次代码已落地：唯一 Rust 契约、DTO 生成、能力协商、Mock/Tauri、页面、revision 冲突、扫描中拒绝更新、保持身份。前端 38 tests / lint / typecheck / build 与浏览器 Mock 交互通过；真实原生选择/持久化未验收。 |
| P0 | Rust 与真实 IPC 闭环 | 本机缺 link.exe/MSVC/SDK，用户决定不安装。需有编译环境后跑 Core/contracts/adapters/desktop tests、clippy、native ignored tests；实际启动 Tauri，从 UI 启用→选择目录→扫描→查缓存→取消→退出→重启→导出，连续三次数据不增长。不得用模拟 IPC 或 Node SQL 替代此证据。 |
| P0 | 签名后 sidecar 身份 | 当前坚持锁中原始 SHA。macOS ad-hoc/Developer ID、Windows Authenticode 可能改变字节；需设计并验证可信来源与签名后 manifest/OS 校验联结，再交给数据 runner 接入。不能现场计算一个值就当作预期值、不能静默绕过原校验。当前签名后包内运行未验证。 |
| P1 | Windows 安装与包内运行 | 有 MSVC/SDK 的原生 x64 环境构建 NSIS；安装后验证固定 sidecar、无 Node/Bun/Rust 依赖、空格中文路径、重启/升级 DB 保留、WebView2 缺失条件及最低 OS。记录主程序/sidecar/安装器/安装后体积与 SHA。staging 检查不算安装验收。 |
| P1 | macOS ARM64 安装与包内运行 | 在 Apple Silicon 主机执行 CLI/runner fixture，构建 .app/DMG、可执行权限/签名结构/安装/退出回收；验证 WKWebView 与候选最低 OS。当前没有任何 Mac 执行证据；Intel/universal 不在范围。 |
| P1 | 完整许可与构建材料 | ccusage MIT 已从锁定 SRI tarball提取并随资源打包。补 Rust/npm/native transitive notices 库存、构建 commit/工具链/runner image/版本/大小/源哈希与最终哈希清单、体积解释。 |
| P1 | CI 实际运行和集中资产门槛 | YAML/两目标矩阵/完整 Action SHA 已静态检查，workflow 尚未执行。native runner 与包内 fixture 已接入；必须等待两平台验证成功后统一汇总必需资产，不让单平台生成可发布结果。 |
| P2 | 用户交互与诊断扩展 | 当前设置已支持基础项；数据目录备份/清除、采集版本元数据、精细扫描订阅仍未开放。分别定义接口和验证，不混入统计实现。 |

用户已于 2026-10-04 授权创建 GitHub public repo 并首发 v0.0.1 源码预览；当前没有安装器资产。正式签名凭据和生产操作仍另行确认。所有未通过平台/安装项保持未验证；本文件不代表桌面软件已达到 Definition of Done。

Windows 编译工具就绪后，可按 data.md/architecture.md 执行；本次不再次申请安装，不启动新的 agent，也不修改数据实现或两份原始交付记录。
