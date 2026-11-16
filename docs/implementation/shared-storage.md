# 通用存储能力提取

## 结果与职责

通用实现移至相邻 qdlrs fork 的 `qdl/src/storage.rs`，通过 `qdl::storage` 公开。fork CLI 的命名分区读写与清零调用公共库。产品的 GPT 校验、分区枚举和备份分区读取复用公共实现；底层 `storage::program_partition` 辅助函数也委托公共库，但实际 restore／flash 仍通过产品写入执行器调用 `firehose::program_with_mode`，保留设备模式、传输统计、验证和停止策略。公共分区写入测试不代表产品 restore／flash 执行路径的测试。已有产品设备适配改动保留。

- 公共库：`Table`、`Partition`、GPT 读取与校验、唯一名称定位、分区列表、分区读写范围检查、精确源长度和末扇区补零。
- fork CLI：`util.rs` 只保留薄适配和表格输出；dump／dump-part、write、erase 调用公共能力。原始存储覆盖和 rawprogram 继续使用原有范围输入路径。
- 产品：保留安全文件名、目录冲突规则、LUN 发现策略、GPT 捕获、partial 文件、报告、设备专用读取适配及验证／停止策略。`gptman` 直接依赖移至测试依赖。

`read_gpt`、`read_partition`、`program_partition` 显式接收 storage slot，保留 CLI 非零 slot 能力。产品薄适配继续传入 slot 0。

`read_gpt_with` 允许调用方观察两次读取，保留失败数据和逐项结果。回调必须按请求长度追加数据并检查完成响应，不能自行扩大读取范围。公共库校验返回长度，短数据不会进入切片解析。`read_partition_with` 在调用产品读取适配前验证分区几何与范围。

## 行为边界

公共 GPT 路径沿用产品既有约束：512／4096 字节扇区、128 字节分区项、主元数据最多 16 MiB。拒绝头部变化、溢出、CRC 错误、越界及重叠分区。同一 LUN 内名称按大小写精确匹配，精确重名时报错。fork CLI 因此不再选择第一个重名项。

这些约束会拒绝不支持的布局，不证明设备实际容量。库不自动修复 GPT，也不在失败后重试或重启。写入要求非空源，源长度必须能放入目标分区；仅最后一个扇区补零，提前 EOF 保留为失败。回读和会话停止由调用方决定。

## 依赖与发布状态

公共接口已提交为 [`205c7d4d9f15add22d78c422ba77843ab72c95d4`](https://github.com/LanceaKing/qdlrs/commit/205c7d4d9f15add22d78c422ba77843ab72c95d4)，并按用户明确指令推送至 `LanceaKing/qdlrs` 的 `oplus-edl-phase1` 分支。产品 Cargo.toml 与 Cargo.lock 已切回该完整 Git revision，不再依赖相邻工作区。远端与 Cargo 解析来源及重新验证结果见[发布记录](shared-storage-publish/README.md)。

提取与审查阶段曾使用 `path = "../qdlrs/qdl"` 联调；该状态已结束。旧 revision `a80cd1e8cd1cd60634b9c5ce9a4c8c96b727c003` 不提供新接口。提交包含 `Assisted-by`，没有代签 `Signed-off-by`。本次不提交产品仓库的其他在途改动。

## 提取阶段离线验证

本次在 macOS 本地运行，两边均通过格式检查、Clippy（全部目标／功能，拒绝警告）、构建和完整测试。qdlrs 95 个测试通过；产品 135 个测试通过，原有 1 个忽略测试未执行，JSON 输出协议子进程通过。未操作设备或外部服务，未执行 Windows／Linux 原生验收。

8 个通用回归从产品迁至 `qdl/tests/storage.rs`。库另增 3 个测试，覆盖非零 slot／LUN 和两种扇区尺寸、回调短数据、无效分区不调用传输。`cli/tests/storage.rs` 新增 2 个测试，覆盖 CLI 拒绝重名及实际分区读取。产品保留 16 个存储集成／工作流测试，并运行其余完整套件。

命令为 `cargo fmt --all -- --check`、`cargo clippy --offline --all-targets --all-features -- -D warnings`、`cargo build --offline`、`cargo test --offline`。构建、Clippy、测试日志保存在 [shared-storage-verification](shared-storage-verification/)。
