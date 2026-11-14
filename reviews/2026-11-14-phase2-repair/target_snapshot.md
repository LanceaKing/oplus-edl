# 第二阶段复审修复

日期：2026-11-14。针对独立代码复审的 1 项 P1 和 3 项 P2，按用户授权完成本地修复及离线验证。没有提交、推送、枚举、打开或操作设备。没有开放 restore／flash，没有引入清单、数据库、计划审批或设备专用逻辑。

## 修复与回归

| 发现 | 修复 | 离线证据 |
| --- | --- | --- |
| P1：加载初始化可进入 Sahara 内存转储分支 | qdlrs 的 WaitingForImage 模式只允许 Hello、ReadData／ReadData64、EndOfImage 和 DoneResp。其他消息在响应、内存读取、文件创建前报错。新增 `sahara_load_programmer` 作为产品专用入口；原显式 Command／MemoryDebug 工具不改变范围 | fork 覆盖初始和 Hello 后的调试、命令及 reset-response 消息；产品初始化测试断言不发送任何响应，也不尝试继续读取、configure 或 reset。测试只提供消息头，不运行内存导出或文件覆盖 |
| P2：主 GPT 失败只报顶层错误且丢失未完成数据 | 主 GPT 探测和完整读取都经过文件与有界缓冲双写，记录独立结果项、范围、接受字节数和错误。失败保留 `primary.bin.partial`，不发布正式文件，不继续备份 GPT 或分区 | 两个阶段分别覆盖部分数据 EOF、超时、完整数据丢最终 ACK、初始 NAK；另覆盖完整数据的 CRC 错误。断言 partial 精确内容、项内错误及后续命令为零 |
| P2：默认发布强制依赖硬链接 | 替换为独占重命名：macOS `renamex_np(RENAME_EXCL)`、Linux `renameat2(RENAME_NOREPLACE)`、Windows `MoveFileExW` 不设置替换标志。保留显式 overwrite 的替换路径，发布前关闭文件 | 现有完整发布／覆盖测试，加上目标并发创建和重命名失败回归。竞争目标不被覆盖，失败保留完整 partial。未挂载或实测 exFAT；确认生产路径不调用硬链接 |
| P2：未选中大小写相似名称导致拒绝 | 单分区只按精确名称定位；同 LUN 精确重名仍报不支持。文件系统上已有目标仍受原覆盖规则约束 | GPT 同时含 boot／Boot，分别精确选中时读取正确 LBA 并成功；保留精确重名及缺失目标回归 |

GPT 探测使用 LBA 1，一旦进入完整读取，复用本次创建的 partial 文件并从 LBA 0 重新记录。探测阶段失败时 partial 仅含 LBA 1 的实际接受字节，结果项的 `start_byte` 明确起点，不把它描述为完整 GPT。正常备份有四个结果项：头部探测、主 GPT、备份 GPT、分区。成功探测没有单独持久化文件；完整主 GPT 校验与发布完成后才继续分区定位，因此后续目标映射失败也会保留已成功的 GPT 捕获与结果。

独占重命名不使用“先检查再覆盖”的回退，不会用复制创建一个未写完的正式文件。若平台、内核或文件系统拒绝独占重命名，则报错并保留 partial。Windows／Linux 实际文件系统行为仍需后续平台测试。

## 本地 qdlrs 与固定依赖

P1 必须在 qdlrs 消息分派前修复，不能只在 `sahara_run` 返回后判断结果。未在产品中复制 Sahara 解析器或新增生产传输框架。

- qdlrs 基线仍为 `7e0a31a5be2ab5d01b27bd743ee67a445ad0b84d`；本次改动未提交，工作区 `/Users/Apple/Documents/qdlrs` 有 4 个修改文件。
- 产品 Cargo.toml 保留原完整 Git revision。`.cargo/config.toml` 明确、临时地将该 Git 来源的 qdl 覆盖为 `../qdlrs/qdl`；该配置已列入 `.gitignore`，不作为发布依赖。
- 当前产品 Cargo.lock 由 Cargo 实际生成，qdl 条目为本地 path 来源，**不是新的固定 Git 修复版本**。`cargo metadata` 已核对来源，见[dependency-source.json](phase-2-repair/verification/dependency-source.json)。产品构建必须连同这个本地覆盖理解。
- 产品调用新增 `sahara_load_programmer`。隔离副本移除覆盖后，旧 revision 因缺少该 API 编译失败；不会静默构建回有问题的调用路径。见[预期失败记录](phase-2-repair/verification/unpatched-build.json)。

后续发布固定依赖的顺序：

1. 人工复核 qdlrs 的[未提交补丁](phase-2-repair/qdlrs.patch)，按其 AGENTS.md 要求由人工签署并提交、推送。当前没有实际新提交号，本次没有代签 DCO。
2. 确认修复提交能从 `LanceaKing/qdlrs` 获取后，将产品 Cargo.toml 的 `rev` 改为实际完整提交号。
3. 移除 `.cargo/config.toml` 的本地覆盖，运行 `cargo update -p qdl` 更新锁文件；确认 Cargo.lock 和 `cargo metadata` 都指向该 Git 提交，不残留 path 来源。
4. 重跑产品四项检查，再将构建结果作为固定 Git 依赖版本的验收证据。当前本地覆盖检查不能替代这一步。

[原产品锁文件](phase-2-repair/baseline-Cargo.lock)用于对照，不应直接恢复覆盖当前新依赖。原第一、二阶段证据保持为历史记录。

## 检查结果

macOS 26.6.2 arm64，Rust／Cargo 1.99.0。产品 37 个测试通过：CLI 8、device 5、storage 24；qdlrs 工作区 82 个测试通过。两者均通过格式检查、Clippy（全部目标和功能，拒绝警告）、锁定依赖的离线测试及构建。

| 范围 | 格式 | Clippy | 测试 | 构建 |
| --- | --- | --- | --- | --- |
| 产品，本地 qdl 覆盖 | [通过](phase-2-repair/verification/product-fmt.log) | [通过](phase-2-repair/verification/product-clippy.log) | [37 个通过](phase-2-repair/verification/product-test.log) | [通过](phase-2-repair/verification/product-build.log) |
| qdlrs，未提交修复 | [通过](phase-2-repair/verification/qdlrs-fmt.log) | [通过](phase-2-repair/verification/qdlrs-clippy.log) | [82 个通过](phase-2-repair/verification/qdlrs-test.log) | [通过](phase-2-repair/verification/qdlrs-build.log) |

另通过 `x86_64-pc-windows-gnu` 的全部目标／功能交叉 `cargo check`。这不是 Windows 链接、运行或文件系统验证。Linux 构建、Windows 实跑、exFAT 实跑及实机操作均未执行。完整命令与结果见[summary.json](phase-2-repair/verification/summary.json)。本轮未执行新的独立代理复审，不把实施者检查称为独立复审通过。
