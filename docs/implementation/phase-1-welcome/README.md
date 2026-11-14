# welcome 结束标记衔接修复

日期：2026-11-13。按用户指令，先将已复审的第一阶段改动提交为 `bd136c484ba4c21e09c14230d10cd0c8956a2fcf`，再修复前次报告记录的上游既有衔接问题。该提交的 16 个文件与已通过 76 个测试及独立复审的快照逐一匹配，见[提交记录](../phase-1-repair/commit.json)。提交仅在本地，包含 `Assisted-by`，未添加 `Signed-off-by`。

## 修复

welcome 读取遇到 `INFO: End of supported functions` 后立即返回，留下文档后的 CRLF。若下一响应以 XML 声明开始，原解析器将 CRLF 拼到声明前，导致 XML 解析失败。

现有 XML 读取器在每个文档开始前消费空格、制表符、CR 和 LF。此规则同时用于 welcome 与命令响应。解析器仍在 `</data>` 处返回，不消费其后的原始存储数据。welcome 结束标记仍立即结束收集，不增加等待、重试或状态字段。命令模式的超时和 EOF 仍失败；部分 XML、非法字节和超过大小限制的 XML 仍失败。

生产行为改动只有 `firehose_read_xml` 中消费文档前空白的条件，另更新相关注释、README 和 AGENTS 指引。没有新增依赖、生产抽象或设备操作。

## 回归与检查

新增 5 个 Firehose 测试，覆盖：

- 结束标记、混合 XML 空白、带声明的 configure 响应在所有二段分片及逐字节分片下衔接成功，且保留下一响应。
- 衔接后读取全为空白字节的原始数据，验证所有分片位置下数据逐字节保留，并正确读取最终 ACK。
- 仅有空白或部分 XML 后超时／EOF，不能代替命令 ACK。
- 非 XML 空白及完整但畸形的 XML 仍报错。
- 文档前空白不计入文档内容，XML 本身仍受原有大小限制。

修复生产代码前，50 个 Firehose 测试中 47 个通过、3 个失败；失败分别复现 configure 衔接、raw 数据读取前衔接及带空白的精确 XML 长度边界。见[修复前日志](verification/before-fix.log)。修复后，完整工作区 81 个测试全部通过：7 个核心单元、50 个 Firehose、12 个 Sahara、4 个选择测试、8 个 CLI 输入测试。

| 检查 | 结果 |
| --- | --- |
| 格式检查 | [通过](verification/fmt.log) |
| Clippy：全部工作区、目标、特性，警告视为错误 | [通过](verification/clippy.log) |
| 测试：全部工作区、特性 | [81 个通过，0 个失败](verification/test.log) |
| 构建：全部工作区、特性 | [通过](verification/build.log) |

Cargo 检查使用 `--offline --locked`，格式检查不涉及依赖解析。命令、退出码与版本见[检查摘要](verification/summary.json)。验证在 macOS arm64 上完成，不代表物理设备或 Windows／Linux 运行验证。

实施完成后按用户要求做了[独立代码复审](../../../reviews/2026-11-13-welcome-code/consolidated_review_report.md)：未确认需要修复的 P0–P3 问题，81 个测试重跑通过。复审确认请求范围内的原始数据不会被空白处理裁剪，完整 ACK 仍为必需条件；它不保证能识别错误发送端的真实数据意图。一个最初提出的“7 字节＋CRLF”疑点经独立对照后撤回，报告保留分歧与理由。

独立复审完成后，按用户要求将这 4 个文件提交为 `7e0a31a5be2ab5d01b27bd743ee67a445ad0b84d`，见[提交记录](commit.json)。提交内容与已复审和检查的快照一致。随后按用户要求与 `bd136c4` 一起推送至 fork 的新分支 `oplus-edl-phase1`，远端提交已确认，见[推送与核验记录](../phase-1-publish/README.md)。qdlrs 工作区干净，未枚举、打开或操作设备。原[独立补丁](qdlrs.patch)、[文件摘要](patch-state.json)、提交取证、评审报告和快照保持归档，其中未提交／未推送状态为取证时的历史值。
