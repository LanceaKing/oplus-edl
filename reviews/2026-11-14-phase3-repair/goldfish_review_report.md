# 第三阶段修复独立复审

日期：2026-11-14。结论：前轮 5 项 P2 均已闭合。本轮在修复差异及关联路径中未确认新的需修复问题，未发现失败停止行为回归。此结论限定于第三阶段代码与离线检查，不表示第四阶段写入功能或实机支持已完成。

## 范围与独立性

以 [docs/designs/oplus-edl.md 快照](source/docs/designs/oplus-edl.md)为标准，对照[前轮报告](../2026-11-14-phase3-code/goldfish_review_report.md)、[修复记录](source/docs/implementation/phase-3-repair.md)及[完整修复差异](source/docs/implementation/phase-3-repair/changes.patch)。当前源码／测试快照、完整行数和 SHA-256 记录于 [review_scope.json](review_scope.json)。

三名审阅者均使用独立新上下文 `fork_turns: none`，未继承实现过程对话，均已实际返回 completed。使用 codebase_details persona，并按代码复审任务检查回归测试，不要求增加设计文档细节。审阅者静态只读检查，协调者在 source 快照运行四项离线检查。共享文件系统及宿主指令不属于隔离承诺；本轮没有检索历史或记忆。

| 审阅者 | 分配范围 | 状态与结论 |
| --- | --- | --- |
| repair_mapping | 目录选择、共享冲突规则、FIFO 输入与相关回归 | completed；两项原问题 closed，无确认的新问题 |
| repair_sparse | 稀疏展开对齐、零长度块、XML 输入与范围检查 | completed；稀疏和 FIFO 相关问题 closed，无确认的新问题 |
| repair_results | 预检结果、文本标识、失败停止、阶段边界 | completed；两项原问题 closed，无确认的新问题 |

## 五项闭合结论

| 原问题 | 结论 | 代码和回归证据 |
| --- | --- | --- |
| firmware-1-1：合法稀疏输入被拒绝 | **closed** | [sparse.rs](source/src/sparse.rs#L42) 已移除 block 必须整除扇区的条件，改为在类型／payload 校验后检查展开 chunk 长度和偏移；零长度支持类型无操作跳过。`tests/firmware.rs` 验证 512-byte block 的 4096-byte RAW／FILL／DONT_CARE、XML 候选、零长度三种类型及后续文件偏移，错误 payload 和未知类型仍拒绝。声明块数、文件末端及溢出校验保留 |
| directory-1-1：未选中大小写名称阻止恢复 | **closed** | [restore.rs:46](source/src/restore.rs#L46) 仍针对全部目标检查精确名称唯一性；[110–117 行](source/src/restore.rs#L110) 只对已选映射进行大小写冲突检查。共享 helper 继续按 LUN 区分。`selected_case_variant_roundtrips_backup_directory` 验证 boot／Boot 分别备份后，带／不带过滤均恢复到正确起点；精确重名继续拒绝 |
| directory-1-2／failure_stop-1-2：FIFO 阻塞预检 | **closed** | [input.rs:8](source/src/input.rs#L8) 在打开前拒绝特殊文件，Unix 使用 O_NONBLOCK，并在打开后复查描述符。restore、XML 和 program 源均调用它。`tests/input.rs` 对 FIFO、FIFO 符号链接、引用 FIFO 的 XML、FIFO XML 本身运行有期限的子进程，未打开写端；普通镜像仍接受 |
| failure_stop-1-1：预检误标执行失败 | **closed** | [output.rs:102](source/src/output.rs#L102) 的 reject 保留错误类别并使用 not_attempted；backup 的名称冲突、输出存在、GPT 大小预检已改用它。实际读取失败仍用 fail。workflows 测试核对全异常／混合异常／已存在输出、原文件保留、命令数量和后续读取行为 |
| failure_stop-1-3：文本缺少异常分区标识 | **closed** | [output.rs:188](source/src/output.rs#L188) 每项包含 ID、LUN、分区名和路径；名称和路径带转义，source 缺失不再丢失分区标识，原因／错误关联 item ID。workflows 测试检查 boot、Boot、../bad 标识，以及真实读取失败和后续停止原因 |

## 新问题与停止行为

本轮没有确认新增 P1／P2／P3 问题，没有待修复发现。审阅并核对以下约束：

- 真实读错误仍保留已完成结果与失败 `.partial`，不继续本 LUN 剩余读取或进入下一个 LUN；未新增 reset、重试或重连。
- `Item::reject` 不改变错误汇总优先级，不将 unsupported／input_error 转成成功；实际读失败仍报告 failed。
- `src/main.rs:47–49` 仍在配置、加载器读取和设备枚举前拒绝 restore／flash；CLI 边界测试继续通过。
- Cargo.toml、Cargo.lock、main.rs、cli.rs、device.rs 与修复前评审快照一致；没有修改固定 qdl 依赖。

## 本轮独立重跑

在当前 macOS 主机的本轮 source 快照执行，使用锁定依赖与离线模式。每项退出码均为 0。

| 检查 | 结果 |
| --- | --- |
| 格式检查 | [通过](verification/fmt.log) |
| Clippy，全部目标／功能，拒绝警告 | [通过](verification/clippy.log) |
| 全部测试 | [58 个通过](verification/test.log)：CLI 8、device 5、firmware 8、input 3、storage 24、workflows 10 |
| 全部功能构建 | [通过](verification/build.log) |

准确命令及返回码见 [summary.json](verification/summary.json)。58 个测试函数中包含一个 FIFO 子进程入口，普通全量运行时它在没有测试环境参数时返回；FIFO 实际检查由父测试启动四个子进程完成。

## 验证限制

两项非阻断覆盖空白保留，不将它们当作已发现缺陷：

- 同时选中 boot／Boot 的拒绝通过共享冲突 helper 验证，未用同一目录中的两个实际文件端到端验证；生产目录映射调用链已静态核对。
- 没有确定性注入 metadata 检查与 open 之间的路径替换竞态；O_NONBLOCK 和打开后类型检查的衔接经静态核对，现有 FIFO 无写端场景实际通过。

未执行 Windows／Linux 平台验收或设备测试。本轮只有一次独立复审，不将其描述为两轮收敛。未修改生产代码、原测试或既有设计／实施记录；仅新增本报告、快照及验证日志。未提交、推送或操作设备。源码未变化核验见 [integrity-check.json](integrity-check.json)。
