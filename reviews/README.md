# Oplus EDL 评审报告

最新代码结论见 [welcome 独立复审](2026-11-13-welcome-code/consolidated_review_report.md)：未确认需要修复的 P0–P3 问题；请求范围内的原始数据完整保留，ACK 条件未放松，81 个工作区测试重跑通过。一个最初报告的 P1 经字节边界对照后撤回，原证据和裁定均保留。

实施状态：已复审提交 `bd136c4` 和 `7e0a31a` 已按用户要求推送至 [fork 的新分支 `oplus-edl-phase1`](https://github.com/LanceaKing/qdlrs/tree/oplus-edl-phase1)。远端提交已核验，[固定依赖记录](../docs/dependencies/qdlrs.md)已更新。qdlrs 工作区干净，未操作设备。[推送记录](../docs/implementation/phase-1-publish/README.md)与下述历史评审版本分开保存。

第一阶段修复后的最新结论见[独立复审汇总](2026-11-13-phase1-repair/consolidated_review_report.md)：前次 3 个 P2 已闭合，76 个测试及四项检查通过，未发现新的修复问题或范围扩张。报告单列一个上游既有的 welcome 衔接边界。未提交、未推送、未操作设备。

前次代码复审见[2026-11-13 汇总](2026-11-13-phase1-code/consolidated_review_report.md)：当时确认 3 个 P2，原报告与快照保留。下述通过结论仅指设计文档。

最新结论见[独立复审汇总](2026-11-13-rereview/consolidated_review_report.md)：首轮五项主要问题在设计层面均已闭合，未发现新矛盾或首版范围扩张。该结论不代表生产实现、构建或硬件验证已经通过。

两轮均采用 EGM 完整连续模式，各包含五个阶段和九次独立上下文评审。

| 轮次 | 汇总报告 | 结论 | 审阅版本 |
| --- | --- | --- | --- |
| 2026-11-13 首轮 | [首轮汇总](2026-11-13-initial/consolidated_review_report.md) | 架构符合范围，实施前需解决 5 项主要问题，另有 10 项次要意见 | [237 行快照](2026-11-13-initial/design_snapshot.md) |
| 2026-11-13 复审 | [复审汇总](2026-11-13-rereview/consolidated_review_report.md) | 文档复审通过，阻断、主要、次要发现均为 0 | [375 行快照](2026-11-13-rereview/design_snapshot.md) |

## 阶段报告

| 阶段或记录 | 首轮 | 复审 |
| --- | --- | --- |
| 5：整体架构检查 | [报告](2026-11-13-initial/zoom_out_report.md) | [报告](2026-11-13-rereview/zoom_out_report.md) |
| 6：文档理解检查 | [报告](2026-11-13-initial/comprehension_report.md) | [报告](2026-11-13-rereview/comprehension_report.md) |
| 7：设计、结构与文字评审 | [报告](2026-11-13-initial/critic_report.md) | [报告](2026-11-13-rereview/critic_report.md) |
| 8：第二次整体检查 | [报告](2026-11-13-initial/intermediate_zoom_out_report.md) | [报告](2026-11-13-rereview/intermediate_zoom_out_report.md) |
| 9：实施就绪检查 | [报告](2026-11-13-initial/implementation_readiness_report.md) | [报告](2026-11-13-rereview/implementation_readiness_report.md) |
| 范围、上下文与哈希 | [记录](2026-11-13-initial/review_scope.json) | [记录](2026-11-13-rereview/review_scope.json) |

## 归档说明

报告中的行号对应同目录的 `design_snapshot.md`，不是后续修改的设计文件。两份快照的 SHA-256 均与各自 `review_scope.json` 中的 `sha256` 一致。

归档只调整报告位置和 Markdown 链接，未改动评审发现或结论。首轮原来位于项目根目录的六份报告和 `review_scope.json` 现位于 `2026-11-13-initial/`。复审目录保持原位置。

`review_scope.json` 的路径和 `baseline_files` 哈希保留评审时的值，不随归档重写。快照也保留原始内容和当时的引用路径。部分历史路径已迁移，查阅现有报告时使用本索引。

[当前设计文档](../docs/designs/oplus-edl.md)的评审状态和引用入口已同步更新。技术方案正文未因本次归档改变。
