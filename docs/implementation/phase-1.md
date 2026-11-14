# 第一阶段实施记录

日期：2026-11-13。依据：[已复审设计](../designs/oplus-edl.md) 的第一阶段。

第一阶段实现与验收已完成。修复位于 `/Users/Apple/Documents/qdlrs` 的 `oplus-edl-phase1` 分支，上游基线为 `75fe2f7e42546a1d75969040378d4616419a9dcf`。第一阶段已复审改动提交为 `bd136c484ba4c21e09c14230d10cd0c8956a2fcf`；后续 welcome 衔接修复通过 81 个测试、四项检查及独立复审，提交为 `7e0a31a5be2ab5d01b27bd743ee67a445ad0b84d`。两个提交均已按用户要求推送至 [LanceaKing/qdlrs 的新分支 `oplus-edl-phase1`](https://github.com/LanceaKing/qdlrs/tree/oplus-edl-phase1)，远端引用、父子关系和文件树已核验。[固定依赖记录](../dependencies/qdlrs.md)采用最新完整提交号；qdlrs 工作区干净。见[推送与核验记录](phase-1-publish/README.md)。

首轮代码复审发现的 3 个 P2 已逐项修复，该版本的 76 个测试及四项检查通过。三位独立审阅者确认三个问题闭合，未发现新的修复问题或范围扩张，见[独立复审汇总](../../reviews/2026-11-13-phase1-repair/consolidated_review_report.md)。修复内容和证据见[复审修复记录](phase-1-repair/README.md)。下文的 62 个测试、14 个文件及 `phase-1/` 补丁均为首轮历史证据；已提交版本的完整补丁和文件摘要位于 [phase-1-repair/](phase-1-repair/README.md)，后续 welcome 改动单独归档。

## 范围与改动

本阶段修改 fork 的协议路径，保留原有三个 Cargo 工作区成员。未创建产品入口、额外生产传输 trait、数据库、清单、保存计划或设备适配模块。未修改依赖、Cargo.lock、许可证或评审归档。未运行任何设备枚举、打开、上传、读取、写入或重置操作。

| 设计修正项 | 实现与离线证据 |
| --- | --- |
| program 输入与传输 | `read_exact` 完成整块输入后才发送。短读可继续，EOF 和硬错误保留原因。串口续传短写，USB 短完成直接失败。数据完成后仍须最终 ACK |
| read 输出与结束 | 使用 `write_all` 和输出 flush。零进展、EOF、硬错误、NAK、缺失最终 ACK 均失败。合并的 XML／原始数据／后续响应不会互相吞掉 |
| 命令 ACK 与欢迎日志 | `firehose_read_welcome` 单独收集日志。命令等待中的日志、部分 XML、超时或欢迎结束标记均不能代替 ACK |
| 错误与副作用 | `MutationFailure` 区分三种确认状态并保留原因。完整 NAK 含重试提示时仍为 Rejected。错误路径和 Drop 不发送 reset。显式 reset 只发送一次并等待确认 |
| 存储信息 | 返回设备日志和响应属性。NAK 不再返回成功。初始完整拒读使用独立的 `ReadRejected` 类型 |
| Sahara | 检查偏移加长度溢出、主机整数转换、镜像范围、缺失镜像及固定报文长度。设备输入不再触发原有断言。非法完成状态报错 |
| 配置与后端 | 默认 `SkipWrite=0`，保留显式吞吐测试设置。大小必须有效。配置失败不重置，不隐式重配。USB 检查完成状态和实际长度，取消不触发自动重试。串口设置读写超时 |
| 设备选择 | `select_and_open` 要求唯一精确匹配。未指定、未匹配、重复匹配均不调用 open。USB 物理位置、USB 序列号和串口路径接入同一检查 |
| 输出 | `quiet` 控制产品使用的加载和存储路径。子进程回归确认这些路径的 stdout／stderr 无库输出。上游 CLI 只适配受影响接口，保留自身输出和成功后的 reset 策略 |

`qdl/tests/support/mod.rs` 的 `ScriptedIo` 实现既有 `QdlReadWrite`。Read 和 BufRead 共用消费位置。脚本显式注入字节片段、EOF、错误、短写及 flush 故障。共享记录在 Drop 后保留实际接受字节、调用边界、空写和 flush 次数。未提供的读步骤立即使测试失败，不依靠真实超时。

## 验收证据

首次在未修复的上游代码运行四个回归，四个均失败：短读丢数据、提前 EOF 被报成功、日志后超时被报 ACK、EOF 后仍继续读。修复后这些用例全部通过。其余用例覆盖各类错误组合、边界及选择行为。

主机为 macOS 26.6.2 arm64，Rust 1.99.0。最终工作区测试共 62 个通过、0 个失败：核心单元测试 7 个，`firehose_io` 39 个，`sahara_transfer` 12 个，`selection` 4 个。Firehose 数量包含一个用于子进程输出验证的辅助测试入口。上游已有 5 个 Sahara 单元测试继续通过。

| 检查 | 结果与日志 |
| --- | --- |
| `cargo fmt --all -- --check` | [通过](phase-1/verification/fmt.log) |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | [通过](phase-1/verification/clippy.log) |
| `cargo test --workspace --all-features --locked` | [62 个通过](phase-1/verification/test.log) |
| `cargo build --workspace --all-features --locked` | [通过](phase-1/verification/build.log) |
| 核心库不启用后端的编译 | [通过](phase-1/verification/no-backend.log) |
| 核心库仅 USB 后端的编译 | [通过](phase-1/verification/usb-only.log) |
| 核心库仅串口后端的编译 | [通过](phase-1/verification/serial-only.log) |

完整命令、退出码和工具版本见 [verification/summary.json](phase-1/verification/summary.json)。离线结果不证明驱动可用、真实设备兼容或刷写成功。未在 Windows／Linux 运行检查，未运行远端 CI。产品级停止策略、三种回读验证模式及最终动作组合属于后续阶段，不能由本次库测试推导其已经实现。

## 首轮提交准备与固定依赖（历史记录）

fork 的 [AGENTS.md](/Users/Apple/Documents/qdlrs/AGENTS.md) 保留上游贡献规则：“The AI agent must never add a `Signed-off-by` tag”以及“a human must review, sign off, and submit every change.” 因此本次完成代码和验收后保留未提交状态，没有代签 DCO、推送改动或提交 PR。

可审阅的 [完整补丁](phase-1/qdlrs.patch)、[文件与补丁摘要](phase-1/patch-state.json) 和 [提交消息](phase-1/commit-message.txt) 已准备。补丁在临时基线工作区通过应用检查，应用后全部 14 个改动文件与已测试工作区的摘要一致，见[补丁检查日志](phase-1/verification/patch.log)。提交消息包含 `Assisted-by`，不包含 `Signed-off-by`。人工复核后，在 fork 工作区签署并提交这些改动，再推送分支。使用实际生成且可从 fork 获取的提交更新 [依赖记录](../dependencies/qdlrs.md)。产品阶段二再写入 Cargo Git revision，不使用当前上游基线冒充修正提交。
