# 第一阶段 qdlrs 独立代码复审

结论：需要修复后再接受。确认 3 个 P2 问题，未发现 P0／P1 问题或超出已批准精简范围的功能扩张。现有测试和构建通过，但未覆盖下面的调用方与协议边界。

日期：2026-11-13。基线：`75fe2f7e42546a1d75969040378d4616419a9dcf`。审阅补丁 SHA-256：`8e00c89ae3ca79342c7aec7dc8db6a42a616c082943206d94123c0309216e28a`。审阅对象为本地未提交改动及新增测试，不是一个已发布提交。

本轮使用三个独立上下文审阅者，分别检查 Firehose、后端与 Sahara、测试与范围。协调者核验源码并复跑反例。只完成一轮代码复审，没有进入修复循环。真实 fork、设计正文和原实施归档保持不变。未提交、未推送、未枚举或操作设备。

## R1 · P2：严格输入读取没有同步到现有 CLI 调用方

定位：[qdl/src/lib.rs:602](/Users/Apple/Documents/qdlrs/qdl/src/lib.rs:602)。调用方为 [erase](/Users/Apple/Documents/qdlrs/cli/src/main.rs:345)、[overwrite-storage](/Users/Apple/Documents/qdlrs/cli/src/main.rs:374)、[write](/Users/Apple/Documents/qdlrs/cli/src/main.rs:409) 和 [rawprogram](/Users/Apple/Documents/qdlrs/cli/src/programfile.rs:167)。

`read_exact` 本身是必要修复，但旧调用方仍依赖被移除的隐式补零。`erase` 只传入 `&[0u8][..]`，因此正常分区都会在 program 已发送并收到初始 ACK 后报 `Unknown/UnexpectedEof`。文件写入把长度向上取整为扇区数，却仍传入未补尾的 File。非整扇区镜像因此在数据阶段失败；镜像跨越多个传输块时，前面的块已经发送。

已直接调用当前 `parse_program_xml` 复现：9 字节镜像、4 字节扇区、3 个扇区、8 字节载荷，仅发送前 8 字节便报 EOF，最后一扇区没有发送。擦除来源也复现为一条 program 命令、零原始数据。对固定上游基线执行同样的输入场景，擦除和镜像尾部补零均成功。这证明调用方回归来自新输入契约，不能用“拒绝 EOF 是正确的”解释掉。

最小修复：保留库中的严格读取。擦除调用方显式提供有限零流；允许末扇区补零的文件调用方显式提供已知长度的补尾。已知源区间不合法时在发送 program 前报错。不能恢复对任意提前 EOF 的静默补零。

缺少的回归：通过真实调用方输入准备或 XML 入口测试擦除、小于一扇区的镜像、跨块非整扇区镜像和真正的输入截断，断言完整目标字节及最终 ACK。

来源合并：FH-01、BS-02、COV-01。协调者独立复跑确认，见 [当前行为复现](coordinator/verified-findings.log) 和 [上游基线对照](coordinator/baseline-compat.log)。

## R2 · P2：拒读类型丢失必须停止会话的信息

定位：[qdl/src/lib.rs:313](/Users/Apple/Documents/qdlrs/qdl/src/lib.rs:313) 和 [qdl/src/lib.rs:699](/Users/Apple/Documents/qdlrs/qdl/src/lib.rs:699)。新类型定义位于 [qdl/src/types.rs:60](/Users/Apple/Documents/qdlrs/qdl/src/types.rs:60)。

`NAK + AttemptRestart="1"` 跳过了重启提示的错误分支。`firehose_read` 随后丢弃响应属性，读取路径把它转换成无字段的 `ReadRejected`。明确携带 `rawmode="true"` 的 NAK 也得到相同类型。调用方因此不能区分普通的、仍在命令边界的拒读和要求重启或明确仍处于原始数据模式的响应。

[设计第 122 行](design.md) 只允许在完整 NAK 且会话仍处于命令边界时继续 best-effort 操作。阶段一不需要实现产品继续策略，但必须保留实现该策略所需的信息。这里的缺陷是新拒读接口和重启处理丢失状态，不是要求本轮新增产品工作流。

两个输入均已通过实际 `firehose_read_storage` 复现，返回错误都能转换成 `ReadRejected`。探针没有执行后续设备命令；未来调用方可能错误继续的影响来自接口分析，并非已经运行了产品流程。

最小修复：修改操作仍保留完整 NAK 对应的 `Rejected`，读路径则保留或检查会话状态。至少不能把要求重启或明确 `rawmode=true` 的响应返回为普通可继续拒读。无需自动 reset、重试或新增会话框架。

缺少的回归：普通拒读仍可区分；重启／重试提示与显式原始数据状态必须保留停止信息。现有 hint 测试只证明库不自动发送命令，没有验证提供给上层的继续条件。

来源：FH-02；协调者独立复现，见 [验证日志](coordinator/verified-findings.log) 与 [期望行为反例](coordinator/review-probes.log)。

## R3 · P2：欢迎日志的合法尾部空白被误判为截断

定位：[qdl/src/lib.rs:233](/Users/Apple/Documents/qdlrs/qdl/src/lib.rs:233)，尤其是第 235 行的 `pending.is_empty()`。真实 CLI 在 [main.rs:293](/Users/Apple/Documents/qdlrs/cli/src/main.rs:293) 调用该路径。

加载器发送完整日志 XML，以 `</data>\n` 或 `</data>\r\n` 结束，没有特殊的 “End of supported functions” 日志，随后进入空闲。解析器先返回完整日志，把尾部空白留给下一次读取。下一次读取把空白放入 pending，于是空闲超时被当作未完成 XML 返回错误，CLI 无法继续 configure。

当前快照已复现 LF 和 CRLF 的失败。固定上游基线的欢迎日志读取接受相同的完整文档加 CRLF、超时场景。问题在新增欢迎模式的边界判定；要求命令响应必须有 ACK 的修复仍应保留。

最小修复：仅在欢迎文档之间识别 XML 空白，将只有这类空白的 pending 视为文档边界。包含未完成标签时继续报错。不要在原始存储数据路径无条件丢弃空白字节。

缺少的回归：LF、CRLF、空格、分片空白后空闲超时成功；空白加未完成标签仍失败；普通命令等待中的仅日志加超时仍不能变成 ACK。

来源合并：FH-03、BS-01、COV-02。协调者独立复跑确认，见 [当前行为复现](coordinator/verified-findings.log)、[上游基线对照](coordinator/baseline-compat.log) 和 [CRLF 探针](coverage_scope/probe-results.txt)。

## 测试充分性与范围结论

已有测试有效覆盖了多数直接缺陷：可继续的短读短写、提前 EOF、硬错误、零进展、初始／最终 NAK、丢失 ACK、输出 flush 失败、XML 分片、Sahara 越界、明确选择和静默输出。`ScriptedIo` 的 Read／BufRead 共享位置，读脚本耗尽会失败，共享写入记录在 Drop 后仍可检查。

主要空白是库与实际调用方的输入契约、欢迎文档边界，以及拒读结果对上层的语义。通过纯完成长度／错误转换函数的 USB 单元测试，不等同于验证真实 USB 控制器。此次源码核对支持直接检查完成长度、取消不自动重试及移除空读 drain 的方向，未建立额外可复现的后端或 Sahara 缺陷。

改动仍在已批准的第一阶段范围内，没有数据库、清单、审批计划、补丁模拟器、产品工作流或无关生产 trait。R1 要求完成现有调用方适配，R2 和 R3 要求修复本阶段新增边界，均不扩大产品功能。不要求本轮补齐阶段 2–6 的验证模式、工作流、跨平台运行或设备测试。

返回 configure NAK 为失败是已批准修正，没有将“恢复隐藏的配置重试”列为问题。上游 CLI 原有的成功后 reset 和自动 bootable 策略没有作为本次新增缺陷。协调者额外探测到通用 ACK 解析仍不校验 rawmode，但该行为在基线已存在，未单独算作本次新增问题；本报告的 R2 针对新拒读接口丢失继续条件。

## 实际执行与复审记录

协调者在真实工作区重跑以下检查，全部退出 0：格式检查、Clippy（全部工作区、目标、特性，警告视为错误）、62 个现有测试、完整工作区构建。命令与日志见 [checks.json](coordinator/checks.json)、[fmt](coordinator/fmt.log)、[Clippy](coordinator/clippy.log)、[test](coordinator/test.log)、[build](coordinator/build.log)。

新增审阅探针仅放在各自的隔离副本中，不属于原工作区测试。故障特征探针断言当前错误行为，其“通过”表示复现成功。期望正确行为的探针失败是反例证据，不能混入原有 62 个测试的结果。上游对照的三个兼容性探针全部通过。

| 审阅者 | 状态 | 报告 | 合并后的问题 |
| --- | --- | --- | --- |
| `/root/review_firehose` | completed | [Firehose](firehose/report.md) | R1、R2、R3 |
| `/root/review_backend_sahara` | completed | [后端与 Sahara](backend_sahara/report.md) | R1、R3 |
| `/root/review_coverage_scope` | completed | [覆盖与范围](coverage_scope/report.md) | R1、R3 |

三位审阅者均使用 `fork_turns="none"`，实际返回了报告。没有传入父对话或其他审阅结果。文件系统仍共享，主机指令注入的排除未另行验证，不宣称强隔离。输入摘要、范围和完成状态见 [review_scope.json](review_scope.json)。本次只更新本复审目录和评审索引，未修改实现。
