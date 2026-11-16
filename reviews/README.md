# Oplus EDL 评审报告

这里保存各轮评审结论、受审源码快照和复现证据。当前实施进度见[实施索引](../docs/implementation/README.md)。每份报告只适用于其受审版本；历史报告中的“未提交”“未操作设备”和测试数量不代表当前工作区状态。

| 范围 | 初审 | 修复后复审或后续记录 |
| --- | --- | --- |
| 设计 | [首轮汇总](2026-11-13-initial/consolidated_review_report.md) | [独立复审](2026-11-13-rereview/consolidated_review_report.md) |
| 第一阶段协议 | [代码复审](2026-11-13-phase1-code/consolidated_review_report.md) | [修复复审](2026-11-13-phase1-repair/consolidated_review_report.md) |
| 第一阶段 welcome 衔接 | [独立复审汇总](2026-11-13-welcome-code/consolidated_review_report.md) | [实施记录](../docs/implementation/phase-1-welcome/README.md) |
| 第二阶段修复 | [复审报告](2026-11-14-phase2-repair/goldfish_review_report.md) | [修复记录](../docs/implementation/phase-2-repair.md) |
| 第三阶段 | [代码复审](2026-11-14-phase3-code/goldfish_review_report.md) | [修复复审](2026-11-14-phase3-repair/goldfish_review_report.md) |
| 第四阶段修复 | [复审报告](2026-11-14-phase4-repair-rereview/goldfish_review_report.md) | [实施记录](../docs/implementation/phase-4.md) |
| 第五阶段 | [第一轮](2026-11-14-phase5-review/goldfish_review_report.md) | [第二轮](2026-11-14-phase5-review/round-2/goldfish_review_report.md) |
| 云端刷写与整包安全修复 | [第一轮报告及修复结果](2026-11-16-cloud-flash-repair/goldfish_review_report.md) | 7 项已修复；第二轮尚未启动，新增路径未做实机重测 |

设计复审闭合了首轮五项主要问题。第一阶段修复复审闭合了三个 P2，第三阶段修复复审闭合了五个 P2。具体边界以各报告为准，不能将评审通过等同于实机验收。

下表的阶段编号是设计评审流程编号，与产品实施的六个阶段不同。

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

清理时仅删除未纳入 Git 的 `target/` 编译缓存。报告、日志、补丁、源码快照和探测用例保留原路径；需要复现时重新构建。
