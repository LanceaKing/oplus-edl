# 第一阶段复审修复记录

日期：2026-11-13。基线为 `75fe2f7e42546a1d75969040378d4616419a9dcf`。本轮依据[第一阶段代码复审报告](../../../reviews/2026-11-13-phase1-code/consolidated_review_report.md)修复 R1、R2、R3，不扩展产品阶段。

| 问题 | 修复 | 离线回归 |
| --- | --- | --- |
| R1：调用方未提供全部扇区字节 | CLI erase 使用有长度上限的全零输入；write、overwrite 和 rawprogram 共用文件区间读取器，只补齐最后一个非整扇区。无效偏移、越界区间和溢出在该 program 命令前报错；检查后发生的文件截断仍报错 | 新增 `cli/tests/program_input.rs` 的 8 个测试，调用生产输入适配器和 rawprogram 解析入口。覆盖对齐与非对齐文件、跨 payload 尾部补零、有限擦除输入、带偏移的选中区间、无效区间无命令、截断时不补造数据及最终 ACK 缺失 |
| R2：拒读类型丢失命令边界条件 | 初始完整 NAK 仅在没有重试／重启提示，且 rawmode 缺省或为 false 时返回 `ReadRejected`。其他情况要求停止会话。修改操作的完整 NAK 仍为 `Rejected` | 覆盖普通 NAK 后保留下一响应、带提示或 rawmode 异常的 NAK 不可降级、无恢复命令，以及 program 初始／最终 NAK 的确认状态 |
| R3：welcome 尾部空白被误判为未完成 XML | 只在 welcome 文档边界消费 XML 空白；部分 XML 仍报错，命令等待和原始数据读取不裁剪空白 | 覆盖 LF、CRLF、空格／制表符、分片空白、带声明的连续 XML、空白后的部分 XML／非法字节、命令日志后超时，以及 raw 数据开头的空白保真 |

新增生产代码限于一个 CLI 输入适配模块和两处现有协议边界修复。没有新增生产传输 trait、产品入口、持久化计划、状态框架或自动重试。保留原工作区成员、依赖锁文件、许可证和 CLI 成功后策略。rawprogram 的范围检查在各自 program 命令前进行；本阶段没有实现产品层的整批执行预检。

## 检查

| 命令 | 结果 |
| --- | --- |
| `cargo fmt --all -- --check` | [通过](verification/fmt.log) |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | [通过](verification/clippy.log) |
| `cargo test --workspace --all-features --locked` | [76 个通过，0 个失败](verification/test.log) |
| `cargo build --workspace --all-features --locked` | [通过](verification/build.log) |

测试由 62 个增至 76 个：核心单元测试 7 个，`firehose_io` 45 个，`sahara_transfer` 12 个，`selection` 4 个，CLI `program_input` 8 个。Firehose 数量仍包含一个用于子进程输出验证的辅助测试入口。命令、退出码和版本见 [summary.json](verification/summary.json)。检查在 macOS arm64 上执行，未在 Windows／Linux 或真实设备上验证。

完整补丁见 [qdlrs.patch](qdlrs.patch)，本轮改动摘要见[复审 delta](../../../reviews/2026-11-13-phase1-repair/repair.diff)。补丁已在临时上游基线目录中通过应用检查；应用后 16 个改动文件与已测试工作区的 SHA-256 一致，见[补丁检查](verification/patch.log)和[状态记录](patch-state.json)。上一轮补丁和评审快照保留为历史记录。

## 独立复审

三位独立审阅者已完成输入适配、协议边界、覆盖与范围检查，确认 R1、R2、R3 闭合，未发现新的修复问题或范围扩张。见[独立复审汇总](../../../reviews/2026-11-13-phase1-repair/consolidated_review_report.md)。额外探针确认一个上游既有的 welcome 结束标记／CRLF／XML 声明衔接问题，报告已单列；没有将其误计作本轮回归，也未扩展本轮修复范围。

本轮复审完成时未提交、未推送，未枚举、打开或操作设备。随后用户明确要求提交，已将本记录对应的代码提交为 `bd136c484ba4c21e09c14230d10cd0c8956a2fcf`，见[提交记录](commit.json)。原补丁和状态摘要保留当时取证值。之后的 welcome 衔接修复单独记录在[后续修复](../phase-1-welcome/README.md)，通过独立复审后提交为 `7e0a31a5be2ab5d01b27bd743ee67a445ad0b84d`。两个提交后续按用户要求推送至 fork 的新分支，见[远端核验记录](../phase-1-publish/README.md)；始终没有设备操作。
