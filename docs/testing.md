# 测试状态

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
