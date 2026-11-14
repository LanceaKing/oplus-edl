# 第三阶段独立代码复审

日期：2026-11-14。状态：一轮审阅完成，发现未修复。未修改产品代码、原有测试或设计文档，未操作设备。

结论：确认 5 项 Major／P2 问题，无已确认 Blocker／P1。不能据此判定第三阶段实现完全符合设计。公开 restore／flash 在设备访问前返回不支持，符合阶段边界；第四阶段尚未实现的验证、写入和最终动作不列为缺陷。

## 范围与方法

标准：[设计快照](source/docs/designs/oplus-edl.md)，全文 1–377 行。审阅目标为第三阶段相关生产代码及测试，完整文件范围和 SHA-256 见 [review_scope.json](review_scope.json)。行号对应本目录 source 快照；报告生成时已核对产品源文件未变化。

按用户指定的三个重点，使用 EGM codebase_details persona 分别执行独立审阅。三名审阅者均以 `fork_turns: none` 启动，不继承本次实现对话；全部返回 completed，并确认读取 persona、设计及相关源码。文件系统共享，未声称隔离了宿主注入的指令。审阅者只做静态只读检查；主协调者在 probe 副本中重跑原测试和最小复现，不修改审阅对象。

| 审阅者 | 重点 | 状态 | 原始结论 |
| --- | --- | --- | --- |
| directory | 目录映射、LUN、名称和预检 | completed | FIFO 输入阻塞；指出大小写往返不对称，但认为设计措辞需要裁定 |
| firmware | XML、稀疏、范围及参考兼容性 | completed | 稀疏对齐检查过严；同项记录零长度 chunk 被拒绝 |
| failure_stop | 失败停止、partial、结果和阶段边界 | completed | 预检执行状态错误、FIFO 阻塞、文本输出缺少异常分区标识 |

FIFO 两次发现合并。名称不对称由协调者复现后依据设计 245、249、251 行确认为问题；审阅者原有保留意见在下文完整保留。没有把同一问题重复计数。

## 问题与建议

### firmware-1-1 — Major / P2：稀疏解析拒绝参考实现支持的已对齐输入

- **位置：** [sparse.rs:42–48](source/src/sparse.rs#L42)，以及 [69 行](source/src/sparse.rs#L69)。
- **触发：** 稀疏块大小为 512，唯一 RAW chunk 包含 8 块，实际传输 4096 字节，设备扇区为 4096。写入起点和长度均对齐，但块大小检查直接返回 `invalid sparse geometry`。
- **另一表现：** 零块 DONT_CARE 后跟一个合法 RAW chunk，也因 `bytes == 0` 被拒绝。
- **影响／依据：** 不能产生合法固件候选操作。设计 100–104 行要求对齐固定 qdl 稀疏行为；已核验参考提交 `c72e7d152acd605de19b64ad4a605ae5b58a7cda`。其 `src/program.c:119–126` 跳过零长度 chunk，并检查展开后的 chunk 长度是否为扇区倍数，而不是要求 sparse block 本身为扇区倍数。
- **AI Suggestion：PROPOSE FIX。** 在结构和 payload 校验后，按展开 chunk 长度／偏移检查扇区对齐；支持合法零长度 chunk 的无操作语义。保留现有非对齐实际写入拒绝规则。
- **证据：** `reproduces_aligned_sparse_rejection` 同时复现两种拒绝，普通 4096 block 对照成功。

### directory-1-1 — Major / P2：目录恢复对未选中的大小写相似分区也判冲突

- **位置：** [restore.rs:110–113](source/src/restore.rs#L110)，对照 [backup.rs:247–260](source/src/backup.rs#L247)。
- **触发：** 同 LUN 的 GPT 含 `boot`、`Boot`。只备份 `boot` 成功，输出只有 `lun0/boot.bin`。再对同一布局进行目录恢复，即使显式选择 `boot`，仍报 `target directory name collision`。
- **原因：** 备份对选中集合检查大小写冲突；恢复对全部 targets 检查，包含未选中的 `Boot`。
- **影响／依据：** 工具自身合法生成的单分区目录无法往返。设计 245 行要求备份和恢复共享命名／匹配规则，249 行要求精确匹配，251 行将大小写冲突限定在选中名称。
- **AI Suggestion：PROPOSE FIX。** 每项先按完整目标集合验证精确名称唯一，再对本次选中的目录映射集合检查大小写冲突。同 LUN 精确重名仍应拒绝。
- **裁定记录：** directory 审阅者指出“恢复拒绝名称不唯一目标”的后半句和现有测试可能支持严格解释，因此最初未正式定级。协调者认为这里 `boot` 的精确目标唯一，`Boot` 未被选择；不能用实现自身的严格测试覆盖设计的选中集合和共享规则要求。该项为协调者复现后确认的 P2，并非伪称三名审阅者一致结论。
- **证据：** `reproduces_backup_restore_case_asymmetry` 确认备份返回 success 后，恢复预检拒绝。

### directory-1-2 / failure_stop-1-2 — Major / P2：FIFO 在普通文件检查前阻塞预检

- **位置：** [restore.rs:18–21](source/src/restore.rs#L18)、[program_xml.rs:160–165](source/src/program_xml.rs#L160)。
- **触发：** Unix 下将无写端的 FIFO 作为单镜像输入，或在 program XML 中引用它。
- **原因：** 两条路径均先执行阻塞式 `File::open`，然后才检查 `metadata.is_file()`。因此普通文件拒绝分支无法及时到达。
- **影响／依据：** 已实现的离线预检 API 无限等待，不返回输入错误；公共写入尚未开放，不涉及当前设备修改。第三阶段记录承诺普通文件检查，设计要求必要输入检查和首次修改前拒绝无效输入。
- **AI Suggestion：PROPOSE FIX。** 打开前拒绝非普通文件，保留打开后的描述符校验；需要覆盖路径替换竞态时采用合适的非阻塞打开方式。新增无写端 FIFO 回归，不能依靠外部写端解锁后才结束测试。
- **证据：** `reproduces_fifo_block_before_type_check` 对两条 API 分别等待 300ms，均没有结果；随后打开写端，两者立即进入类型拒绝分支。复现主动释放并回收线程，未留下阻塞任务。

### failure_stop-1-1 — Major / P2：纯预检拒绝被记录为执行失败

- **位置：** [backup.rs:269–272](source/src/backup.rs#L269)、[309–319 行](source/src/backup.rs#L309)，以及 [output.rs:102–107](source/src/output.rs#L102)。
- **触发：** 选中分区名称不安全、名称冲突，或镜像输出已存在。
- **原因：** 这些分区尚未发送读取请求，却调用 `Item::fail` 将 `execution` 设为 `failed`。
- **影响／依据：** JSON 不能准确区分“预检拒绝、未执行”和“实际读取失败”。设计 285 行明确要求单纯预检拒绝使用 `not_attempted`。总体 unsupported／input_error 类别应继续保留，不能改成成功。
- **AI Suggestion：PROPOSE FIX。** 单独记录预检错误并保留 `execution: not_attempted`，实际执行失败才调用执行失败 setter；补充项级状态断言。
- **证据：** `reproduces_preflight_failed_status_without_read` 仅记录两条 GPT 读取，无分区读取，但异常分区输出 `execution: failed`、`transferred_bytes: "0"`。

### failure_stop-1-3 — Major / P2：文本结果丢失失败分区的 LUN 和名称

- **位置：** [output.rs:174–188](source/src/output.rs#L174)，异常项来源见 [backup.rs:263–273](source/src/backup.rs#L263)。
- **触发：** GPT 中有不安全名称，或 `boot`／`Boot` 等选中冲突项，同时还存在可成功备份的其他分区。
- **原因：** 不安全／冲突项没有 source，文本输出回退为操作名 `read`；后面的错误也没有对应的 item ID、LUN 或分区名。
- **影响／依据：** 用户只看到 `read failed (0 bytes)` 和通用错误，无法识别哪些分区没有备份。设计 80 行要求逐项列出 LUN、扫描范围和失败项；文本结果应提供可识别的逐项结果。JSON 中仍保留标识，问题限于默认文本展示。
- **AI Suggestion：PROPOSE FIX。** 文本每项显示 LUN、分区名及执行状态，并将原因／错误绑定到该项。异常文件名不能用于创建路径，但可以作为文本标识显示。
- **证据：** 同一预检 probe 调用 `Report::emit(false)`，输出第 3 项为 `read failed (0 bytes)`，缺少 `../bad` 分区标识。

## 失败停止与未发现问题的范围

本轮未发现读取失败后继续发命令或隐式 reset 的缺陷。既有测试覆盖 EOF、ACK 缺失、超时、partial 保留和后续读取停止；多 LUN 场景中前一个 LUN 失败后不进入后续 LUN。已有成功项不会被后续失败清除，整体结果不会因此成为 success。

XML 文件／节点顺序、非空缺文件拒绝、源偏移和尾部补零、已知目标溢出，以及 DONT_CARE 不计入传输量，在审阅范围内未确认新增缺陷。非 DISK 补丁先校验后跳过、表达式子集等候选疑点未取得足够具体的兼容性失败证据，不列为已确认问题。此结论不是证明所有输入均正确。

## 验证与边界

- [原有测试重跑](baseline-tests.log)：50 个通过，未修改原测试。
- [最小复现源文件](probe/tests/review_probes.rs)、[输出](probe-results.log)：4 个探针通过。这里“通过”表示成功复现当前缺陷，不表示修复后验收。
- 源码快照和探针分开保存；新增内容仅在本评审目录。
- 没有执行新一轮独立修复复审，也没有运行硬件、跨平台或实机测试。

按用户“先报告问题，不修改代码”的要求，本轮停在报告阶段。上述 PROPOSE FIX 仅为建议，尚未应用；本报告不表示已批准修复或风险接受。
