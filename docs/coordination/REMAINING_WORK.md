# 当前功能与剩余工作

更新：2026-10-07，America/Phoenix。当前应用 **0.0.7 / API 1.2.0 / profile v4**，支持 Windows x64、macOS Apple Silicon ARM64。

**开发中：** 分支 `feat/follow-system-timezone` 在本地实现系统时区跟随（应用 **0.0.8 / API 1.3.0 / profile v5**，[T10](TODO_GUIDE.md#t10)）。前端/Node/契约检查已在本机通过；本机无 Rust 工具链，Rust 编译/测试/clippy、两平台 CI、新安装包与真机验证均未执行。[设计与本地验证](../ci-validation/follow-system-timezone-v1.md)

0.0.7 安装包代码：`465ac24ef7e99d5e465d5e9b495ce9d5672084b0`；[最终 CI 37672597908](https://github.com/upuphero/agent-usage-dashboard/actions/runs/37672597908) 五个 jobs 全部成功。后续纯文档提交不改变这份安装包的代码身份。[验收证据与 SHA](../ci-validation/auto-full-scan-v1.md) · [TODO 详细含义与完成标准](TODO_GUIDE.md)

## 已实现并通过相应验证

| 项目 | 当前能力与证据边界 |
| --- | --- |
| 三个本地来源 | Claude Code、Codex sessions/archived_sessions、Antigravity 已支持的 SQLite .db；.pb 尚不支持，缺失字段保持不可用 |
| 本地统计 | SQLite 完整 Daily/Session 快照、日/周/月趋势、来源/模型分布和排序、会话累计量与分页、覆盖范围/精度/stale 提示；token/价格口径未改 |
| 来源与设置 | 启用/关闭、目录选择和不透明引用、revision 冲突校验、扫描期间来源/时区写入门禁、身份/历史保留；原生 GUI 选择器的人机操作仍需 T1 验收 |
| 时区与语言 | 0.0.7：新配置使用系统本地时区，旧默认 UTC 按既有迁移规则重扫；中文默认、英文切换与显示偏好保留。0.0.8 分支（未经 CI）：“跟随系统 / 固定时区”两种模式、原生检测与安全重建，旧配置迁移为固定 |
| 手动扫描与导出 | 同来源合并、取消/失败保留历史、退出回收；JSON 完整历史归档及筛选 CSV；导入、用户备份/恢复/清除 UI 尚未实现 |
| 自动完整扫描 v1 | 默认关闭，1/5/15 分钟（默认 5）、metadata/文件身份/WAL/原生监听、Rust 公平串行调度、合并/限流/退避、恢复检测、取消、后台事件与缓存刷新；仍是完整扫描，三个 incremental 能力均为 false |
| 构建与安装检查 | NSIS、portable ZIP、ARM64 DMG 已生成；CI 实际安装/解压、包内 sidecar 原始 SHA/架构及 fixture 通过，三份下载包本地重算 SHA 通过 |
| 验证数量 | 67 前端、16 Node/SQL、43 Linux Rust；两平台各 75 默认 + 5 native tests，安装后各再跑 5 native；typecheck/lint/build、契约/边界/版本和严格 clippy 均通过 |

CI 证明的是代码、合成输入与受控 runner 场景；不能据此把真实 GUI、硬件休眠、干净机/用户数据升级或最低 OS 标为通过。

## 剩余 TODO 与建议顺序

P0 是当前测试包的真实使用验收，P1 是稳定分发准备，P2 是后续功能；不是必须一次全部完成，也不代表已排定发布日期。

| 优先级 | ID / 工作 | 完成出口 |
| --- | --- | --- |
| P0 | [T1 真机桌面 UI/IPC](TODO_GUIDE.md#t1) | 两平台实际点击完成启用→选目录→扫描→图表/会话更新→取消→退出/重启→导出；中英文一致，并记录系统与包 SHA |
| P0 | [T2 自动扫描真机场景](TODO_GUIDE.md#t2) | 最小化、真实睡眠跨多个周期、持续写入/WAL、关闭/取消/退出；不补跑错过周期、不丢新变化、不残留进程 |
| P0 | [T3 安装、升级与兼容](TODO_GUIDE.md#t3) | 干净机/WebView2 缺失、0.0.6→0.0.7 配置/身份/历史保持、安装/便携共享、macOS 下载隔离属性和最低 OS 场景通过 |
| P1 | [T4 长期下载与版本发布](TODO_GUIDE.md#t4) | 滚动开发渠道与固定版本附件、精确提交/校验材料、失败不覆盖好包；目前只有一天 Actions artifacts 与 v0.0.1 源码预览 Release |
| P1 | [T5 正式签名/公证](TODO_GUIDE.md#t5) | Windows 发布者签名/时间戳、macOS Developer ID/公证、sidecar 信任方案及实际下载验证；账号/证书/凭据操作单独授权 |
| P1 | [T6 完整第三方许可](TODO_GUIDE.md#t6) | 对随包分发的 Rust/npm/native 依赖生成并人工核对 notices；当前只完成 ccusage 完整 MIT 与基础声明 |
| P2 | [T7 真正增量采集](TODO_GUIDE.md#t7) | 按来源设计游标/稳定身份、修正/轮转/截断处理、事务与全量复核；不能直接把变化文件再累加 |
| P2 | [T8 数据维护](TODO_GUIDE.md#t8) | 用户可备份、恢复、清除与迁移应用数据；有预览、版本/身份校验和失败恢复；底层 SQLite 备份测试不等于用户功能完成 |
| P2 | [T9 归档导入/多设备/独立数据集](TODO_GUIDE.md#t9) | 严格校验归档、重复导入幂等、revision/来源冲突与重叠提示；明确数据集切换，不按设备名简单求和 |
| P2 | [T10 系统时区跟随](TODO_GUIDE.md#t10) | **本地实现完成，待 CI 与真机**：两平台 Rust 编译/测试/clippy/fmt、跨午夜 native 重建、新包，以及运行中切换系统时区、扫描中切换、取消/重启恢复与 0.0.7 升级的真机证据 |
| P2 | [T11 自定义日期/项目统计](TODO_GUIDE.md#t11) | UI 日期选择、日期边界/分页/导出一致；有可信项目元数据与 unknown 状态，再增加项目维度 |
| P2 | [T12 托盘/开机启动/通知/更新](TODO_GUIDE.md#t12) | 分别定义生命周期、独立开关和平台验证；更新还需独立签名、版本/迁移/失败恢复 |
| P2 | [T13 Antigravity .pb](TODO_GUIDE.md#t13) | 主 conversation .pb 的版本化 usage schema、fixture、隐私边界及平台验证；现有附属 metadata protobuf 过滤不等于 .pb 来源支持 |
| P2 | [T14 订阅额度](TODO_GUIDE.md#t14) | 可信账户级来源、含义/重置时区/更新时间与 unknown 状态；不从 token/API 等价成本推导剩余额度 |
| P2 | [T15 更多来源](TODO_GUIDE.md#t15) | ChatGPT Web、DeepSeek Harness、Cowork 等逐个核实数据、授权范围和统计口径，再提供独立 Adapter/fixture/开关 |
| P2 | [T16 诊断展示扩展](TODO_GUIDE.md#t16) | 展示可信采集/归一化版本与更细的处理进度；已有 scan/auto 状态订阅不重做，不虚构百分比或泄露原文/路径 |

建议先 T1→T2→T3，再按分发需求安排 T4–T6；P2 按实际使用痛点独立排期，不回填为当前能力。

## 后续边界

本机不安装 MSVC/SDK，原生构建沿用已通过的现有标准 Windows/macOS CI；不上传私人日志，不把合成测试当成真实日志/GUI 验收。代码或当前文档更新的授权不自动扩大为新 Release、旧 tag/附件覆盖、凭据变更或正式签名授权。历史版本的 `docs/ci-validation/` 与 `docs/releases/` 记录保留当时事实，当前状态以本文件和 0.0.7 最终证据为准。
