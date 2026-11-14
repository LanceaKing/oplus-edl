# 第三阶段复审修复

日期：2026-11-14。按用户授权修复[独立复审报告](../../reviews/2026-11-14-phase3-code/goldfish_review_report.md)中的 5 项 P2，并补充回归测试。未提交、推送或操作设备。公共 restore／flash 仍在设备访问前返回不支持，没有接通第四阶段流程。

## 修复与验证

| 复审问题 | 修复 | 回归证据 |
| --- | --- | --- |
| firmware-1-1：稀疏几何限制过严 | 按展开 chunk 长度及偏移检查扇区对齐。先验证类型及 payload，再忽略合法零长度 chunk | 512 字节 block 组成 4096 字节 RAW／FILL／DONT_CARE，通过解析及 XML 候选生成；非对齐实际 chunk 仍拒绝；合法零长度三种类型不产生范围，错误 payload／未知类型仍拒绝 |
| directory-1-1：未选中大小写名称阻止恢复 | 精确目标唯一性仍针对全部 targets 检查，大小写冲突只针对本次选中映射。备份和恢复继续共用 storage 名称冲突函数 | GPT 同时含 boot／Boot，分别单独备份并从生成目录恢复映射成功；精确重名仍拒绝，选中两种大小写的共享冲突检查仍拒绝，跨 LUN 规则保持 |
| directory-1-2／failure_stop-1-2：FIFO 打开阻塞 | 新增内部 open_regular，在打开前检查路径类型，打开后检查描述符类型；Unix 设置 O_NONBLOCK，避免检查后替换为 FIFO 导致阻塞。用于恢复镜像、XML 本身和 program 源文件 | 无写端 FIFO、指向 FIFO 的符号链接、XML 引用 FIFO、FIFO 作为 XML 输入均在子进程限时内返回错误；测试不打开写端，超时会终止并回收子进程；普通文件仍接受，目录拒绝 |
| failure_stop-1-1：预检拒绝误标执行失败 | 新增 Item::reject；名称／输出冲突和备份 GPT 大小预检保留错误类别，execution 为 not_attempted；实际读取失败仍为 failed | 纯异常名称、异常与正常分区混合、已有镜像输出均检查项级状态和命令数；已有镜像保持原内容，其他可读分区继续；真实读失败仍停止剩余读取 |
| failure_stop-1-3：文本缺失异常分区标识 | 文本每项显示 ID、操作、LUN、分区名、源路径、执行状态和接受字节数；原因与错误关联 item ID。名称／路径以带转义的引号形式显示 | 混合结果逐项断言 boot、Boot、../bad 的标识及错误关联；实际读失败的错误和后续停止原因同样关联对应项；JSON 契约与退出类别保持 |

回归位于 `tests/firmware.rs`、`tests/workflows.rs`、`tests/input.rs`。原有 `tests/cli.rs` 的公开工作流禁用断言继续通过。输入测试包含一个供子进程调用的测试入口；正常全量运行时该入口不执行额外操作。

## 全部本地主机检查

当前 macOS 主机检查结果：58 个测试通过（CLI 8、device 5、firmware 8、input 3、storage 24、workflows 10），较修复前新增 8 个测试函数，并扩充原测试断言。

| 检查 | 结果与日志 |
| --- | --- |
| cargo fmt --all -- --check | [通过](phase-3-repair/verification/fmt.log) |
| cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings | [通过](phase-3-repair/verification/clippy.log) |
| cargo test --workspace --all-features --locked --offline | [58 个通过](phase-3-repair/verification/test.log) |
| cargo build --workspace --all-features --locked --offline | [通过](phase-3-repair/verification/build.log) |

机器可读结果：[summary.json](phase-3-repair/verification/summary.json)。相对复审快照的代码／测试差异：[changes.patch](phase-3-repair/changes.patch)；修复文件哈希：[changed-files.json](phase-3-repair/changed-files.json)。Cargo.toml、Cargo.lock 和固定 qdl revision 未改变，没有修改 qdlrs。

这是实施者修复与离线验证记录，不是新一轮独立复审通过结论。未运行 Windows／Linux 平台验收或任何设备测试；第五、六阶段边界保持。原复审报告、源快照及缺陷复现日志保留为修复前证据。
