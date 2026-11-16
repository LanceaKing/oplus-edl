# qdlrs 依赖记录

当前依赖：通用存储能力已发布为 [`205c7d4d9f15add22d78c422ba77843ab72c95d4`](https://github.com/LanceaKing/qdlrs/commit/205c7d4d9f15add22d78c422ba77843ab72c95d4)，位于 `LanceaKing/qdlrs` 的 `oplus-edl-phase1` 分支。Cargo.toml 与 Cargo.lock 固定完整 Git revision，临时路径依赖已移除。见[发布与验证记录](../implementation/shared-storage-publish/README.md)。下文阶段一／二／五的状态与 revision 均为历史记录。

**第五阶段补充（2026-11-14）：** 产品代码现已提交并推送至公开仓库，固定 qdlrs revision 未变。Windows、macOS、Linux 的 GitHub Actions 原生检查全部通过，见[第五阶段记录](../implementation/phase-5.md)。下文“产品代码未提交”等描述为依赖发布时的历史状态。

**当前状态（2026-11-14）：** 第二阶段 P1 修复已提交为 `a80cd1e8cd1cd60634b9c5ce9a4c8c96b727c003`，推送至 `LanceaKing/qdlrs` 的 `oplus-edl-phase1` 分支，并从远端重新拉取核验。产品 Cargo.toml 和 Cargo.lock 均固定新提交，本地 override 已移除。产品四项检查和 Windows 交叉编译检查通过，qdlrs 工作区干净。产品代码未提交，未操作设备。见[发布及固定依赖核验](../implementation/phase-2-publish/README.md)。

以下为第一阶段和第二阶段初始集成的历史记录；表中的旧 revision 不代表当前产品依赖。

本记录对应设计第一阶段，日期为 2026-11-13。两项已复审修复已推送至 fork 并从远端重新拉取核验；后续产品固定引用下表的完整 Git revision，不使用浮动分支。

| 项目 | 值 |
| --- | --- |
| Fork | https://github.com/LanceaKing/qdlrs |
| 上游 | https://github.com/qualcomm/qdlrs |
| 固定上游基线 | `75fe2f7e42546a1d75969040378d4616419a9dcf` |
| 本地工作区 | `/Users/Apple/Documents/qdlrs` |
| 本地分支 | `oplus-edl-phase1` |
| 远端分支 | [LanceaKing/qdlrs:oplus-edl-phase1](https://github.com/LanceaKing/qdlrs/tree/oplus-edl-phase1) |
| 固定 Git revision | [`7e0a31a5be2ab5d01b27bd743ee67a445ad0b84d`](https://github.com/LanceaKing/qdlrs/commit/7e0a31a5be2ab5d01b27bd743ee67a445ad0b84d)；包含 welcome 衔接修复，已确认可从 fork 获取 |
| 前一提交 | [`bd136c484ba4c21e09c14230d10cd0c8956a2fcf`](https://github.com/LanceaKing/qdlrs/commit/bd136c484ba4c21e09c14230d10cd0c8956a2fcf)；第一阶段协议修复及 3 个 P2 修复，已在同一远端分支 |
| 工作区状态 | 干净；当前提交与已通过 81 个测试、四项检查及独立复审的代码一致 |
| 许可证 | 保留上游 BSD-3-Clause，未改许可证或版权头 |
| Cargo 依赖与锁文件 | 保持基线版本，无新增依赖，无锁文件变更 |

两项修复按用户要求推送到新远端分支 `oplus-edl-phase1`。`git ls-remote` 确认分支头为固定 revision；另在空临时 bare 仓库拉取该分支，确认 `7e0a31a` 的父提交为 `bd136c4`，再往前是固定上游基线，且两个提交的 tree 与本地一致。完整哈希、核验方式和日志见[远端核验记录](../implementation/phase-1-publish/remote-verification.json)。

第一阶段和 welcome 的本地提交取证分别见[提交记录](../implementation/phase-1-repair/commit.json)与[最新提交记录](../implementation/phase-1-welcome/commit.json)。这些记录中的未推送状态保留为当时的历史值。原补丁、摘要和复审快照不回写；当前远端状态以本记录及远端核验记录为准。产品 Cargo 集成属于下一阶段，本次只更新固定依赖记录。

修复涉及完整输入读取、完整输出写入、传输完成长度、ACK／NAK、原始错误保留、无隐式 reset、Sahara 请求边界、明确选择设备及可控输出。`MutationFailure` 提供 `NotSent`、`Rejected`、`Unknown`，避免产品解析错误字符串。`ReadRejected` 只表示原始数据前收到完整 NAK，且不含重试／重启提示，rawmode 缺省或为 false；其他读取失败要求停止会话。存储信息以日志字符串和响应属性返回，不要求产品截取控制台输出。

`firehose_program_storage` 要求调用者提供全部扇区字节。fork CLI 已使用有限全零流完成 erase，并对文件输入显式补齐最后一个非整扇区；检查后发生的文件截断仍报错。产品允许的末扇区补零由后续产品适配层显式提供。当前工作区的 XML 读取器消费文档前的 XML 空白，允许 welcome 结束标记后衔接带声明的响应；不消费 ACK 后的原始数据，不把部分 XML 或命令超时当作成功。配置响应不再自动重配或重置。收到配置 NAK 时，调用者必须处理失败。USB 按实际完成长度判断写入，并将取消映射为超时，防止 `write_all` 自动重发。串口短写可以继续，硬错误或零进展必须失败。

USB 提供物理位置标识的枚举与打开接口。USB 和串口打开路径共用 `select_and_open`。测试只向该函数传入计数闭包，不枚举或打开硬件。

本阶段只验证 macOS arm64 的构建和离线行为。Windows／Linux 执行检查、产品工作流、验证策略及实机验证仍按设计的后续阶段实施。详细记录见 [第一阶段实施记录](../implementation/phase-1.md)。

## 第二阶段产品集成

2026-11-13：产品 Cargo.toml 已采用上述完整 Git revision，Cargo.lock 已由 Cargo 实际解析生成。`cargo metadata --locked --offline` 确认 qdl 来源为 `git+https://github.com/LanceaKing/qdlrs?rev=7e0a31a5be2ab5d01b27bd743ee67a445ad0b84d#7e0a31a5be2ab5d01b27bd743ee67a445ad0b84d`。没有 path patch，也没有改动 fork。

产品锁文件独立解析传递依赖，不声称与 fork 工作区锁文件完全相同；本次构建和 31 个产品测试验证的是产品锁文件对应组合。结果见[第二阶段记录](../implementation/phase-2.md)。上面的“产品集成属于下一阶段”保留为第一阶段的历史状态。
