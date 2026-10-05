# GitHub Actions 原生编译与打包

仓库必须保持 public。只用免费标准 GitHub-hosted runner：

最新 0.0.3：[Actions run 37261635910](https://github.com/upuphero/agent-usage-dashboard/actions/runs/37261635910)，commit `3e6cb88b2e75fe2e99aff07c5debd0ce3fa65066`，集中 artifact `desktop-installers-3e6cb88b2e75fe2e99aff07c5debd0ce3fa65066`。Windows 免安装 ZIP/NSIS、Mac ARM64 DMG 全部通过，本地重新校验三份文件 SHA。[证据与使用](ci-validation/0.0.3-chart-readability.md)。旧验收记录保留，不修改既有 tag/Release。

| 工作 | runner | Rust target | 安装器 |
| --- | --- | --- | --- |
| 契约、前端、Core、Adapter 验证 | ubuntu-24.04 | runner 原生 | 无 Linux 桌面产品 |
| Windows | windows-2022，x64 | x86_64-pc-windows-msvc | NSIS `.exe` |
| Apple Silicon | macos-15，ARM64 | aarch64-apple-darwin | `.dmg`，薄 ARM64 `.app` |

不使用 larger runner、Intel Mac、universal、跨架构编译、付费证书或自托管机器。
官方标准 runner 的 public repo 运行分钟数免费：[runner 列表](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)。
Actions artifact 存储仍与账户/Packages 共用额度：[计费说明](https://docs.github.com/en/billing/concepts/product-billing/github-actions)。本流水线不启用 cache，只上传必要安装器/证据（每个安装器上限 40 MiB），保留一天；不修改账户计费设置，不能替账户其他仓库保证存储费用为零。

## 触发与产物

`package.yml` 在代码/config 的 main push、pull request、`v*` tag、workflow_dispatch 触发；纯 Markdown/许可/ignore 改动跳过自动构建。它先复用 `ci.yml`，检查锁文件、生成契约、依赖边界、版本、Node/前端/Rust tests 与 clippy，再运行两平台构建。`ci.yml` 也可以单独手动触发。

```text
gh workflow run package.yml --repo upuphero/agent-usage-dashboard --ref main
gh run list --repo upuphero/agent-usage-dashboard --workflow package.yml
gh run download <run-id> --repo upuphero/agent-usage-dashboard --name desktop-installers-<full-commit-sha>
```

tag 必须与 package.json、Cargo workspace/lock、Tauri 和前端显示版本一致。当前 v0.0.1 tag 保持原始源码预览提交；新的 main 构建用 manifest 标记实际 commit，不覆盖 tag，不把 main 安装器附到旧 tag。

两平台都成功才运行 collect。集中 artifact 包含：

- Windows NSIS 和 macOS ARM64 DMG。
- Windows x64 portable.zip，解压后直接运行；独立 portable-manifest.json 验证解压后的主程序/sidecar。
- 各平台 `bundle-manifest.json`：从实际安装器得到的主程序/sidecar 架构、固定 hash、fixture 运行结果和 runner image。
- 各平台 `build-info.json`、ccusage MIT notices。
- `SHA256SUMS`（相对于解压后的集中目录）、`size-report.json`。

只有单平台成功时保留该平台调试 artifact，不生成集中安装包。下载应在完成后一天内进行；过期后可重新手动构建。工作流权限 contents:read，不创建 Release、不改 tag、不导入任何签名凭据。

## 验证边界

Windows 在临时 runner 上真实静默安装 NSIS，验证中文/空格路径里的已安装 binary 和 sidecar。macOS 挂载实际 DMG，复制 `.app` 到中文/空格路径，验证嵌套签名结构、固定 sidecar SHA 和 fixture。两平台随后用安装位置的 sidecar 再执行 Adapter/host native 合成测试，覆盖 SQLite、重复扫描和重启。

Tauri 生产构建显式启用 custom-protocol 嵌入前端，所有 Cargo 构建使用 --locked。macOS 通过官方 macOS.files 将已验证的上游 sidecar 原样放入 Contents/MacOS；保留其原签名和固定 SHA，不接受打包时临时产生的 hash 作为来源预期。主程序和 `.app` 使用 ad-hoc 签名。

Windows 无 Authenticode；macOS 无 Developer ID/公证。CI 安装验证不等于下载后 Gatekeeper/SmartScreen、真实 UI/IPC、WebView2 缺失场景、升级和最低 OS 已通过。13.0 是 macOS 构建目标；最低兼容系统、完整 transitive notices 和真实用户日志验收仍待完成。
