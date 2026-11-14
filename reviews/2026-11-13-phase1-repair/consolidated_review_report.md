# 第一阶段修复后的独立复审

日期：2026-11-13。结论：前次报告的 R1、R2、R3 三个 P2 已闭合。本轮未发现新的 P0–P3 修复问题，未发现超出已批准精简范围的实现。该结论限于本次代码修复与离线证据，不表示所有上游兼容性问题或设备支持已经解决。

输入为本目录 [source/](source/) 的固定快照，[source.patch](source.patch) 是相对上游 `75fe2f7e42546a1d75969040378d4616419a9dcf` 的完整补丁，[repair.diff](repair.diff) 是相对前次代码复审快照的本轮差异。补丁 SHA-256 为 `d2c7f72927b4878faf634792aa221021457711f9aa47b33d5964e5c0c4466f12`。快照、审阅者和完成状态见 [review_scope.json](review_scope.json)。所有实现和测试均未提交、未推送。

## 问题闭合

| 前次问题 | 本轮判断 | 证据 |
| --- | --- | --- |
| R1：严格读取与现有调用方不兼容 | 已闭合。四个生产 program 调用点均提供完整有界输入；erase 提供指定长度的零流，文件只补齐末扇区。已知越界在该 program 前失败；后续文件截断仍保留 `UnexpectedEof`，不发送未完成缓冲区 | [输入审阅](input/report.md)，生产适配器、实际 rawprogram XML 入口及 CLI 调用点核对；8 个新 CLI 测试和额外范围／截断探针 |
| R2：拒读类型丢失继续条件 | 已闭合。初始 NAK 的属性参与分类，有 retry／restart 提示或非 false 的 rawmode 不返回 `ReadRejected`。修改操作的完整 NAK 继续保留 `Rejected` | [协议审阅](protocol/report.md)，属性矩阵、响应分片、下一报文保留、无额外命令及初始／最终修改 NAK 测试 |
| R3：welcome 尾部空白引发超时失败 | 已闭合。welcome 在 XML 文档边界只消费 XML 空白；部分 XML 仍失败，命令等待仍要求响应，raw 数据不被裁剪 | [协议审阅](protocol/report.md)，LF／CRLF、分片空白、连续声明文档、部分／非法 XML 与原始空白数据测试 |

CLI 新增输入适配器是严格核心读取契约的必要调用方配套，没有新增产品流程或生产传输抽象。rawprogram 检查是逐项 program 前检查，不是产品层整批预检。Cargo manifests、Cargo.lock 与固定基线一致。既有 CLI 成功后 reset 和 bootable 策略未改变。[覆盖与范围审阅](coverage_scope/report.md)确认这些边界。

## 验证结果

协调者在真实 fork 工作区执行格式检查、Clippy（全部工作区、目标、特性，警告视为错误）、测试和构建，均退出 0。当前 76 个测试通过：7 个核心单元、45 个 Firehose、12 个 Sahara、4 个选择测试、8 个 CLI 输入测试。命令和日志见[检查摘要](../../docs/implementation/phase-1-repair/verification/summary.json)。完整补丁通过临时基线应用检查，16 个改动文件的摘要均与已测试工作区一致。

| 独立审阅者 | 实际结果 | 报告 |
| --- | --- | --- |
| `/root/repair_review_input` | completed；8 个输入测试、45 个 Firehose 测试、4 个额外探针通过，范围矩阵含 12,285 个组合 | [输入适配](input/report.md) |
| `/root/repair_review_protocol` | completed；45 个 Firehose 测试通过，7 个额外边界探针通过；另有 1 个既有问题诊断失败，见下文 | [协议边界](protocol/report.md) |
| `/root/repair_review_scope` | completed；独立重跑全部 76 个测试和 6 个额外探针通过，范围矩阵含 57,200 个组合 | [覆盖与范围](coverage_scope/report.md) |

协议审阅者保留新增测试，仅反向移除 R2／R3 的生产修复，得到 42 个通过、3 个预期失败；这三个失败分别对应拒读分类、尾部空白及文档间空白。恢复修复后 45 个测试全部通过。这证明回归能够检出原缺陷，不能将反向验证中的预期失败计作当前工作区失败。

额外探针仅存在于各审阅者的私有副本，不计入当前 76 个工作区测试。三位审阅者使用 `fork_turns="none"` 和各自任务包，未读取其他本轮审阅者报告。文件系统仍共享，不宣称强隔离。协调者核对了报告、日志、源码及输入摘要后汇总，未将派发任务当作复审完成。

## 已确认的既有边界

一个额外诊断发现：`INFO: End of supported functions` welcome 结束标记后的 CRLF 会留在缓冲区；下一响应若以 `<?xml ...?>` 开头，普通响应解析会报错。该诊断在当前快照失败；审阅者反向应用完整补丁恢复上游后，同样复现了失败，见[上游对照日志](protocol/scratch/baseline-sentinel.log)。协调者另核对了还原核心文件与固定上游内容的一致性。

此问题早于本轮修复，区别于前次 R3 的“没有结束标记、以空闲超时结束 welcome”场景。本轮不把它算作新增发现，也不宣称已经修好。通用 ACK 的 rawmode 状态校验同样仍按前次范围说明保留。没有为此加入通用状态机、自动重试或扩大协议重构。

## 限制与状态

检查限于 macOS arm64 的离线脚本和本地文件；没有执行应用 CLI 的完整设备流程，未在 Windows／Linux、物理 USB／串口或目标设备验证。产品工作流、整批预检、验证策略和实机兼容性仍属于后续阶段。

实际工作区保留在 `/Users/Apple/Documents/qdlrs` 的 `oplus-edl-phase1` 分支，HEAD 仍是固定上游提交。没有创建提交、推送、枚举、打开或操作设备。实现记录与当前补丁入口见[修复记录](../../docs/implementation/phase-1-repair/README.md)。
