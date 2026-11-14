# 第二阶段实施记录

后续更新：本记录及 `phase-2/` 证据保留为首次实施快照。独立复审发现的 1 项 P1、3 项 P2 已完成本地修复，当前状态见[修复记录](phase-2-repair.md)。下面的无 path patch、31 个测试和工作区状态不代表修复后状态。

日期：2026-11-13。依据：[已复审设计](../designs/oplus-edl.md)的实施顺序第 2 项，以及用户指定的固定 qdlrs revision。第二阶段代码与本机离线验收已完成；未提交、未推送、未枚举或操作设备。

## 本阶段交付

- 根目录新建 Rust 产品包、库与五个命令入口。Cargo.toml 和实际生成的 Cargo.lock 均指向 `LanceaKing/qdlrs` 的 `7e0a31a5be2ab5d01b27bd743ee67a445ad0b84d`。Cargo metadata 确认解析来源为该 Git 提交，不使用本地源码覆盖；未修改 qdlrs fork。
- `src/device.rs` 分开枚举、打开和已构造会话初始化。明确唯一选择后才打开；使用固定 fork 的 Sahara、welcome 和 Firehose configure 路径。库日志静默，设备配置 NAK／已识别扇区冲突报错，无自动 reset。
- `src/storage.rs` 读取 GPT、检查 CRC 和几何范围、精确定位唯一分区，提供单分区读写基础。GPT 元数据读取有 16 MiB 上限。分区范围先于 gptman 对齐计算检查，避免异常零起点条目触发其内部 panic。写入基础只补齐源文件末扇区；声明长度之前的 EOF 是错误，不能用零填补。MutationFailure 确认状态保留。
- `src/backup.rs` 接通显式 LUN、单个分区备份，输出 `lun<N>/<partition>.bin` 和独立 `.gpt` 捕获。校验后的主 GPT 从定位缓冲保存；备份 GPT 与分区直接流式写文件。定位阶段失败不发布 GPT 捕获；文件保存阶段失败保留 `.partial`。正式文件须最终 ACK 和文件同步成功后才发布。不覆盖已有文件，除非给出 `--overwrite`；旧 `.partial` 总是保留并拒绝复用。
- `src/output.rs` 提供设计规定的结果字段、十进制字节字符串、类别优先级和非零退出码。部分备份失败不会汇总为成功，后续读取停止。显式选定单 LUN／单分区的结果始终标明覆盖范围受限。
- `restore`／`flash` 公共入口无条件在设备访问前返回不支持，内部写入基础未接入 CLI。`unlock`／`lock` 同样在设备访问前返回不支持。
- README、MIT 文本、第三方 BSD 来源与许可、测试说明及依赖锁文件已补齐。

根目录原本没有 Git 仓库。本次没有执行 git init、commit、push 或 PR 操作；代码以本地新增文件保留。既有评审快照和第一阶段归档未修改。

## 验收结果

主机：macOS 26.6.2 arm64，Rust／Cargo 1.99.0。31 个产品离线测试通过，0 个失败：CLI 8 个、会话适配 4 个、存储与备份 19 个。表中记录对应最终源码，包含零起点 GPT 和同 LUN 精确重名的追加回归情形。

| 检查 | 结果 |
| --- | --- |
| 格式检查 | [通过](phase-2/verification/fmt.log) |
| Clippy，全部目标／功能，拒绝警告 | [通过](phase-2/verification/clippy.log) |
| 全部离线测试，锁定依赖 | [31 个通过](phase-2/verification/test.log) |
| 构建，全部功能，锁定依赖 | [通过](phase-2/verification/build.log) |

完整命令、返回码、工具版本及解析到的 qdl 来源见[summary.json](phase-2/verification/summary.json)。产品源码与新增交付文件摘要见[files.json](phase-2/files.json)。直接及传递依赖的许可元数据见[dependency-licenses.json](phase-2/verification/dependency-licenses.json)。

测试使用 QdlDevice<ScriptedIo> 调用实际协议路径，没有新增生产传输 trait。测试覆盖分片 Sahara 上传、welcome/configure、GPT 边界和内容变化、512／4096 扇区、分区名冲突、短读短写、提前 EOF、硬错误、NAK、最终 ACK 丢失、尾部补零、`.partial` 保留、显式覆盖、混合备份结果及结果优先级。CLI 子进程只执行设备访问前的路径；没有运行会枚举设备的示例命令。

## 后续边界

第二阶段只开放显式 `--lun` 与一个 `--partition` 的备份。全 LUN 发现、存储信息解释、批量目录映射、XML 和稀疏镜像属于第三阶段。当前不会对未扫描 LUN 或完整设备宣称成功。主 GPT 无效时直接失败，不做主备 GPT 修复；备份 GPT 只捕获实际字节，不证明其一致性。

完整写入工作流、三种回读验证模式、停止策略、启动单元设置和显式重启属于第四阶段。保留 `--verify`、`--yes` 和最终动作的参数语法不表示这些功能已开放。Windows／Linux 实际检查和 CI 属于第五阶段；本次只验证本机构建。实机验收仍需另行授权，不存在任何机型支持或可启动性结论。

没有增加清单数据库、持久化计划、哈希审批、断点续作、认证绕过或设备专用方法。未进行独立代理复审；本记录是实施与离线检查证据，不替代之后的代码复审。
