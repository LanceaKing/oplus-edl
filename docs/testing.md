# 测试状态

存储分层更新：通用 GPT 与传输回归迁至 fork `qdl/tests/storage.rs`，CLI 适配由 `cli/tests/storage.rs` 覆盖。产品 `tests/storage.rs` 保留适配集成、目录与备份结果测试。当前固定 qdlrs `205c7d4d9f15add22d78c422ba77843ab72c95d4`，本次 Git 依赖检查结果见[发布记录](implementation/shared-storage-publish/README.md)。

**第一轮代码审查修复（2026-11-16）：** 7 项已接受发现均落实到代码；离线全功能测试 143 passed、1 ignored，独立 JSON stdout 子进程、格式检查、全目标 Clippy 和构建通过。新增测试覆盖 SWM/云端统一时钟、身份 FIFO、GPT 排除与 patch 冲突、super 可执行不变量、OCDT 项目/范围前检、错误 ACK 模式及 USB 未知计数。未访问手机或外部服务；新增设备前检只在脚本协议中验证。见[修复报告](../reviews/2026-11-16-cloud-flash-repair/goldfish_review_report.md)。这不是第二轮审查或新的实机验收。

**云端资产实刷（2026-11-16）：** 351 次 program 和 65 次 DISK patch 均收到 ACK。重启后出现 Android USB，用户已确认进入初始设置。ADB 已确认版本 RMX3800_14.1.0.190(CN01)，sys.boot_completed 和 dev.bootcomplete 均为 1；未完成全量回读。见[实刷记录](implementation/rmx3800-cloud-flash-20261116.md)。以下条目保留各阶段历史状态。

**preview 云端路径对齐（2026-11-15）：** 新增 DIGEST 握手和显式 RAW 文件长度模式；98 个常规离线测试、JSON 协议子进程、Clippy 和构建通过。原生 DLL 的 9 组长度案例与 4 个状态转换通过。未取得 Windows 成功会话云端资产，未实刷；见[对齐记录](research/preview-rmx3800-alignment.md)。

**第六阶段实机检查（2026-11-15）：** 已按用户授权访问 RMX3800。原包 loader、签名摘要表、configure 和 getsigndata 取得相应完成响应；GPT 读取返回拒绝日志，随后超时，没有最终 ACK／NAK。未发送 program、patch、erase 或启动单元修改。手机已恢复原 Android 16 版本，Mac 配件策略已恢复。实现和验证见[实机记录](implementation/phase-6-rmx3800.md)。以下准备阶段的“未操作设备”等表述为历史状态。

**第六阶段准备（2026-11-14）：** 已完成本地条件及公开资源检查，见[实机验收计划](implementation/phase-6-plan.md)。已匿名分段核验 realme 域名上的三版 RMX3800 服务包、加载器哈希及 XML；存在整包输入缺项和待确认的认证／适配条件。尚未枚举或操作设备，设备组合和可写测试范围仍待补齐，实机验收未开始。

认证输入追踪补充见[来源与握手分析](implementation/phase-6-resources/authentication-inputs.md)：参考工具消费用户提供的 digest／sig；16.0.5.740 包内 SVIP 表与两个清单签名尚不能组成已确认完整的会话认证输入。getsigndata／verify 的状态、原始数据交互及签名取得方式仍待补齐；界面成功状态不作为认证通过证据。

[会话协议分析](implementation/phase-6-resources/session-protocol.md)已恢复主要日志字段、208 字节待验消息和固定 RSA 公钥；合法签名来源、完整传输契约及运行时字段有效性仍缺证据，认证准备项继续标记为未通过。

[MSM 研究迁移评估](implementation/phase-6-resources/msm-comparison.md)已将字段差分、响应分层及隐含副作用检查补入计划。旧 OnePlus AES token 与当前 RSA 会话签名不等价，旧许可证／MES 公钥也不相同；未据此新增设备支持。

**第五阶段原生验收完成（2026-11-14）：** [GitHub Actions run 43608522774](https://github.com/LanceaKing/oplus-edl/actions/runs/43608522774) 对提交 `3aaeaba4898b47d3320e6bcb21674fe2f51e958f` 的 Windows x86_64 MSVC、macOS arm64、Linux x86_64 GNU 四项原生检查全部通过。Windows 实际执行 86 个常规测试，macOS／Linux 各 88 个；三平台均通过独立 JSON 协议子进程检查。Rust／Cargo 为 1.100.0，锁定依赖，测试不操作设备。完整证据见[第五阶段记录](implementation/phase-5.md)。以下“进行中”和“未提交”等内容保留为历史状态。

**第五阶段进行中（2026-11-14）：** macOS 原生格式检查、Clippy、构建、88 个测试及新增 JSON 协议子进程检查通过。Windows GNU、Linux musl 的 Clippy、构建和测试程序编译通过，但未执行目标程序。Linux GNU 跨目标 check／Clippy 通过，链接受本机工具链限制。Windows／Linux 原生四项检查尚未执行；CI 已配置但未运行。完整矩阵及日志见[第五阶段记录](implementation/phase-5.md)。下文各阶段描述均为历史状态，不代表当前公开写入仍被禁用。

**第四阶段复审修复（2026-11-14）：** 3 个 P2 已修复，新增 11 个回归测试，88 个离线测试和本地四项检查通过，见[第四阶段记录](implementation/phase-4.md)。以下第二／三阶段状态保留为历史记录。


**最新固定依赖验证（2026-11-14）：** qdlrs 修复已发布为 `a80cd1e8cd1cd60634b9c5ce9a4c8c96b727c003`，产品移除 path patch 后重新通过四项检查和 37 个离线测试。qdlrs 提交前四项检查及 82 个测试再次通过。Windows x86_64 GNU 的全部目标／功能交叉编译检查通过，未执行 Windows 程序；Linux 和 exFAT 实测未执行。详见[发布记录](implementation/phase-2-publish/README.md)。下文保留初始第二阶段测试范围。

第二阶段按[已复审设计](designs/oplus-edl.md)实现。固定 qdlrs revision：`7e0a31a5be2ab5d01b27bd743ee67a445ad0b84d`。产品从 Git 依赖构建，没有改动 fork，没有 path patch。历史行为参考仍为设计所列 qualcomm/qdlrs、bkerler/edl 和 linux-msm/qdl 固定提交。

产品离线测试通过 `tests/support/mod.rs` 的 ScriptedIo 调用实际 qdl Sahara／Firehose 函数。Read 与 BufRead 共用消费位置，记录写调用和实际接受字节，脚本显式提供 EOF、超时和短写。测试不调用设备枚举、打开或真实连接函数。CLI 子进程测试只运行解析错误和设备访问前的不支持／输入错误路径；目标选择测试传入纯内存候选和闭包。

| 文件 | 范围 |
| --- | --- |
| `tests/cli.rs` | 五入口、参数互斥、默认验证模式、固定返回类别、JSON 字段、结果优先级、明确唯一选择、移除功能不接受 |
| `tests/device.rs` | 脚本化 Sahara 上传及 welcome/configure 衔接、分片输入、配置 NAK／扇区冲突、无隐式重启 |
| `tests/storage.rs` | 512／4096 GPT、CRC／几何／变化／重叠检查、唯一分区、单分区读取、GPT 原始捕获、partial／覆盖、短 I/O、写入补零与截断区别、NAK／丢 ACK |

执行四项检查：

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
cargo build --workspace --all-features --locked
```

检查结果见[第二阶段记录](implementation/phase-2.md)和其 `verification` 日志。依赖下载使用网络；测试内容完全离线且未操作设备。当前只在 macOS arm64 执行，Windows／Linux 的实际构建、CI 和设备驱动验证没有完成。

目录映射、XML、稀疏镜像、三种回读模式、完整停止策略及最终动作属于第三／四阶段测试，当前不宣称通过。跨平台四项检查属于第五阶段。后续实机验收须另行授权，并独立记录设备、加载器、固件、主机、后端、具体命令和结果；只支持实测组合，不推广为通用兼容性。

## 第三阶段

目录备份、恢复预检、XML／稀疏解析的测试范围与执行记录见[第三阶段实施记录](implementation/phase-3.md)。公开 restore／flash 仍返回不支持；新增解析测试不代表写入或验证流程已经接通。

第三阶段复审的 5 项 P2 修复及最新本地主机检查见[修复记录](implementation/phase-3-repair.md)：58 个测试通过，公开写入入口保持禁用。

## 第四阶段

`tests/writes.rs` 使用脚本化 Firehose 传输调用实际写入／读取协议函数，覆盖确认状态、三种验证模式、部分传输计数、恢复验证顺序、补零、稀疏范围、最后写入覆盖、补丁未知范围及最终动作。`tests/cli.rs` 增加第四阶段预检和 JSON 分类；所有子进程用例都在设备枚举／打开前结束。没有使用真实设备、设备枚举或端口打开。

本地主机执行格式检查、Clippy、全部离线测试和构建。Windows／Linux 的原生检查仍属于第五阶段，实机验证仍须后续授权。

## SWM automatic enrollment

`cargo test --lib swm` checks captured server root signatures, nonce replay rejection,
identity reuse, ES256 proof verification, and signed response raw-body binding.
The public fixtures in `tests/fixtures/swm` contain signed server time and online
key metadata only. They contain no client private keys, session tokens, or tickets.
Live service checks are not part of the default test suite. A live check uses
`flash --cloud-config ... --cloud-output NEW_DIRECTORY --cloud-fetch-only` and
must not be reported as an on-device flash test. ADB reboot stays outside flash.

本次 macOS 实机验证先启动主机等待，再独立执行 `adb reboot edl`，成功读取 Sahara v3 身份及 48 字节 PK hash。随后运行产品 `flash --cloud-fetch-only`，通过自动 SWM 注册／取票、云端匹配及三项资产的 ticket 和文件校验。profile 11：loader 1,549,988 字节、digest 33,144 字节、sign 4,096 字节。未上传这组 loader，未写入分区。116 个常规测试通过，1 个忽略；Clippy 通过。
