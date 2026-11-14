# 第四阶段实施记录

最新复审修复：3 个 P2 问题已修复，新增 11 个回归测试，全部 88 个离线测试及本地四项检查通过。下文初次实施的 77 个测试记录保留为历史结果；修复详情见本文末尾。

日期：2026-11-14。范围依据 `docs/designs/oplus-edl.md` 第四阶段及本次授权。未提交、未推送、未枚举或操作设备。qdlrs 固定依赖保持 `a80cd1e8cd1cd60634b9c5ce9a4c8c96b727c003`，没有改动 fork 或依赖锁文件。

## 已完成

- 接通公开 restore／flash。恢复读取所选 LUN 的 GPT，检查全部映射后显示摘要并确认一次；`--yes` 可用于非交互执行。刷写先完成 XML 与文件预检，依次执行 program、DISK patch、验证、可选启动单元设置和可选重启。
- 使用已打开的输入文件和有界缓冲传输、比较。短原始镜像仅补齐最后一个扇区；RAW／FILL 产生写入，DONT_CARE 保持跳过。稀疏块各形成实际 program 结果项，保留同一 XML 节点编号。
- 恢复逐镜像写入并验证；刷写按最后写入确定验证区间。完全覆盖项标记 superseded。可定位补丁屏蔽对应字节，未知地址操作使对应 LUN 的相关写入内容无法验证。不会比较补丁前值，也不计算设备端补丁结果。
- 默认 best-effort；只有类型化的原始数据前完整 NAK 才允许拒读后继续。required 在首次修改前拒绝未知预期内容，运行时拒读停止。none 不发送验证读取，明确输出 not_requested。
- 修改 NAK、缺失确认、超时、截断、失步和不一致按设计停止。不自动重试、重连或 reset。最终动作必须显式请求，且前序策略允许；已确认写入在后续失败时保留确认状态。
- 文本和 JSON 使用同一报告，保留跳过项、未尝试项、验证范围和错误优先级。raw 传输计数来自后端实际接受字节，不使用输入预读量。JSON stdout 只有最终对象，摘要和确认提示写 stderr。
- unlock／lock 仍在连接前返回 unsupported。未引入机型方法、清单、保存计划或自动恢复。

## 回归与检查

新增 `tests/writes.rs` 18 个回归测试及一个 CLI 预检测试，总计 **77 个离线测试通过**。测试覆盖三种验证模式、确认丢失、拒读边界、读取失败、回读不一致、短镜像补零、源文件截断、部分传输计数、重叠／稀疏范围、补丁屏蔽、目录映射失败及最终动作。测试使用内存协议脚本调用实际 qdl 函数，没有枚举设备或打开真实端口。

四项本地主机检查均通过：

- `cargo fmt --all -- --check`：[日志](phase-4-verification/fmt.log)
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`：[日志](phase-4-verification/clippy.log)
- `cargo test --workspace --all-features --locked`：[日志](phase-4-verification/test.log)
- `cargo build --workspace --all-features --locked`：[日志](phase-4-verification/build.log)

验证主机为 macOS arm64。Windows／Linux 原生全部检查属于第五阶段；实机验证属于另行授权的第六阶段。离线测试不证明 RMX3800 或其他机型兼容，也不证明固件可启动。

## 第四阶段复审修复

按后续授权修复三个 P2 问题，并核对设计的验证和结果契约。本次没有修改 qdlrs 固定依赖，没有提交、推送或操作设备。

| 问题 | 修复与结果契约 |
| --- | --- |
| 提前停止后，尚未遍历的覆盖项被误报为验证未完成 | 正常验证与停止后的补记共用最终有效范围计算。完全被已确认写入覆盖的项保持 `not_applicable / superseded`，不增加验证错误；部分覆盖项仅报告剩余有效范围。未尝试的后续操作不参与覆盖，失败或确认缺失的修改不作为已确认覆盖处理 |
| 只含跳过项的 XML 丢失逐项记录 | 解析阶段保留全部候选，将“没有可执行存储操作”的拒绝移至执行器预检。最终结果仍为 `unsupported`，每个跳过项保留来源、XML 节点、原因及 `skipped / not_applicable`，不连接设备 |
| 未支持的稀疏类型误归输入错误 | 未支持的类型和版本返回类型化 `Unsupported`，出口为类别 `unsupported`、退出码 3；错误 magic、已支持类型的错误载荷长度等格式错误仍为 `input_error`、退出码 2 |

确认逻辑移至 `Writes::confirm_and_execute`，CLI 与离线测试使用同一路径。摘要、提示和输入通过显式流传入；拒绝、EOF 或输入错误结束后，全部修改项保持未尝试。`--yes` 跳过输入读取，普通确认只询问一次。

新增 9 个执行回归测试和 2 个 CLI 回归测试，补齐本次审查指出的缺口：

- 验证提前停止时尚未遍历的完全／部分覆盖项，以及修改阶段停止后的已确认覆盖、未尝试和未确认覆盖。
- 只含跳过项时三种模式下的完整 JSON；未支持稀疏类型／版本与损坏载荷／magic 的类别、退出码及 JSON 字段。
- 回读内容完全匹配，但最终 ACK 为 NAK、EOF 或超时：明确保持写 ACK，返回验证未完成，并阻断后续写入和最终动作。既有读取失败测试也改为精确类别断言。
- 重启自身 NAK／丢 ACK：分别为执行失败／未知修改，保留前序结果，重启只发送一次。
- 三种模式下的 `MutationFailure::NotSent`：当前项执行失败、写入未尝试，后续动作未尝试，传输记录为空。
- 可定位及未知地址补丁的跨 LUN 隔离。
- 确认拒绝、EOF、肯定确认和 `--yes`：分别断言零修改、一次确认、跳过输入和显式最终动作。

本机最终检查全部通过，共 **88 个离线测试**：

- `cargo fmt --all -- --check`：[日志](phase-4-repair-verification/fmt.log)
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`：[日志](phase-4-repair-verification/clippy.log)
- `cargo test --workspace --all-features --locked`：[日志](phase-4-repair-verification/test.log)
- `cargo build --workspace --all-features --locked`：[日志](phase-4-repair-verification/build.log)

测试仍仅使用临时输入文件、内存协议脚本及设备访问前结束的 CLI 子进程。没有枚举设备、打开端口或进行实机操作。Windows／Linux 原生检查与实机验证的边界不变。
