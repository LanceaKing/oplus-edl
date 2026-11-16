# 实施记录索引

按[设计的六个实施阶段](../designs/oplus-edl.md)统计，前五阶段已完成，第六阶段已有 RMX3800 实刷和启动证据，全量回读验收尚未完成。阶段完成结论对应记录中的版本和测试组合，不自动覆盖当前未提交改动。

| 阶段 | 内容与状态 | 主要记录 |
| --- | --- | --- |
| 1 | qdlrs 协议修正与离线回归，已完成 | [实施](phase-1.md)、[修复](phase-1-repair/README.md)、[welcome 衔接](phase-1-welcome/README.md)、[发布](phase-1-publish/README.md) |
| 2 | 产品入口、适配层与单分区基础，已完成 | [实施](phase-2.md)、[修复](phase-2-repair.md)、[发布](phase-2-publish/README.md) |
| 3 | 目录备份、恢复映射、XML 与稀疏镜像解析，已完成 | [实施](phase-3.md)、[修复](phase-3-repair.md) |
| 4 | 写入、验证、停止策略与结果输出，已完成 | [实施](phase-4.md) |
| 5 | 三平台原生离线检查，已完成 | [实施](phase-5.md)、[原生验收摘要](phase-5-native/summary.json) |
| 6 | 实机验收，部分完成 | [最新实刷记录](rmx3800-cloud-flash-20261116.md)、[早期连接检查](phase-6-rmx3800.md)、[初始计划](phase-6-plan.md) |

最新实机记录：351 次 program 和 65 次 DISK patch 获 ACK；ADB 已确认 Android 14 / RMX3800_14.1.0.190(CN01) 启动完成。全量回读未完成，不能将 ACK 或启动成功当作逐字节验证。原始证据见[汇总](../../runs/live-cloud-flash-recovery/summary.json)和[启动读回](../../runs/live-cloud-flash-recovery/boot-verification.log)。

共享存储能力的后续提取见[实施记录](shared-storage.md)，配套证据位于 `shared-storage-verification/` 和 `shared-storage-publish/`。

同名阶段目录保存补丁、文件摘要和验证日志；`*-repair` 保存修复证据，`*-publish` 保存发布核验。`phase-5-verification/` 是早期本机与交叉编译记录，`phase-5-native/` 才是三平台原生验收证据。`phase-6-resources/` 保存早期资源核查材料。

历史记录保留当时的输入、结论、日期和路径。后续进展从本索引进入；旧记录中的授权范围和“尚未执行”只描述当次工作。日期沿用原记录所注明的时区。评审报告统一从[评审索引](../../reviews/README.md)查阅。
