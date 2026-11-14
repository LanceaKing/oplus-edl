# oplus-edl

原生 Rust Oplus EDL 命令行工具。当前完成[设计](docs/designs/oplus-edl.md)第四阶段：接通恢复、刷写、三种验证模式、失败停止策略、显式最终动作和结果输出。实施范围与离线验证见[第四阶段记录](docs/implementation/phase-4.md)。

公开存储工作流包括目录备份、普通镜像／目录恢复和 XML 固件刷写。恢复和刷写先检查输入，显示摘要，确认一次后执行；非交互使用 `--yes`。`unlock`、`lock` 没有经过验证的机型方法，在连接前返回 `unsupported`。

第五阶段跨平台检查进行中：macOS 原生四项检查通过，Windows GNU 和 Linux musl 交叉编译通过。Windows／Linux 原生测试尚未执行，CI 已配置但未运行；详见[第五阶段记录](docs/implementation/phase-5.md)。

## 构建与离线测试

```sh
cargo build --workspace --all-features --locked
cargo test --workspace --all-features --locked
```

qdlrs 修复已提交并推送，产品固定引用 `LanceaKing/qdlrs` 的 `a80cd1e8cd1cd60634b9c5ce9a4c8c96b727c003`。Cargo.lock 已更新，本地 `.cargo/config.toml` override 已移除。构建不再依赖相邻的 qdlrs 工作区。

新固定依赖的产品检查已通过，产品代码尚未提交，未操作设备。发布及来源核验见[发布记录](docs/implementation/phase-2-publish/README.md)。已有缓存时可增加 `--offline`，首次获取依赖需要网络。验证范围见[测试记录](docs/testing.md)。

## 命令形式

下列命令是使用示例，本次实施没有运行它们。加载器由用户提供。

```sh
# 不传 --device 时仅枚举候选，返回 input_error，不上传加载器。
oplus-edl backup --output backups/session --backend usb --json

# 当前支持：选择分区；可重复 --partition，省略时备份该 LUN 全部已列出分区。
oplus-edl backup --device '<candidate>' --backend usb --loader loader.elf \
  --memory ufs --lun 0 --partition boot --output backups/session

# 恢复／刷写示例；实际执行还需明确 --device 和 --loader。
oplus-edl restore --from backups/session --verify best-effort
oplus-edl restore --partition boot --image boot.bin --lun 0
oplus-edl flash --program rawprogram0.xml --patch patch0.xml
oplus-edl unlock
oplus-edl lock
```

公共参数支持 `--backend usb|serial`、`--device`、`--loader`、`--memory emmc|ufs`、`--lun`、`--sector-size` 和 `--json`。默认使用 USB、UFS 和 4096 字节扇区；eMMC 默认 512。摘要说明扇区值是介质默认值还是用户指定值，不把默认值称为实测值。当前 GPT 路径只接受 512 或 4096，配置响应给出已识别扇区字段时检查冲突。

USB 后端需要主机允许访问目标接口；Windows 的 USB 接口需要适用的 WinUSB 驱动。串口后端使用系统提供的端口路径和对应串口驱动。Linux 需要相应 USB 设备或串口权限，macOS 需要接口未被其他进程占用。本阶段未安装驱动、修改系统权限或验证任何实机后端。

## 输出与限制

目录备份保存 `lun<N>/<partition>.bin`，GPT 原始捕获保存到 `lun<N>/.gpt/primary.bin` 和 `backup.bin`。先写 `.partial`，完整数据和最终 ACK 成功后才发布正式文件。失败保留 `.partial`；不自动续作。已有正式文件需显式 `--overwrite`；已有 `.partial` 即使带此选项也不截断，需选择新目录或由用户处理旧文件。

分区名必须满足设计的可移植文件名规则。未找到或同 LUN 精确重名的分区不会被猜测匹配。单项精确选择不受未选中大小写相似名称影响，已有目标文件仍受覆盖规则约束。主 GPT CRC、条目 CRC、范围和重叠会检查。主 GPT 无效时本阶段报错，不自动从备份 GPT 修复。主 GPT 和备份 GPT 捕获跨度分别限制为 16 MiB。备份 GPT 保存实际读取的字节，不声称它与主 GPT 一致。

主 GPT 探测和完整读取分别记录结果。探测失败时 partial 仅包含从 LBA 1 读取的字节，起点见结果项。默认文件发布采用系统独占重命名，不要求硬链接；若系统不支持该操作，则保留 partial 并报错，不降级为可能覆盖已有目标的操作。

省略 `--lun` 时查询存储信息，识别明确的物理单元数量并扫描这些 LUN。无法识别数量时返回不支持，可显式指定 `--lun`；不通过读失败推断扫描结束。当前识别响应属性 `num_physical_partitions`、`num_physical`、`bNumberLu` 和日志 JSON 的 `storage_info.num_physical`。扫描或读取失败会停止会话；结果保留完成项和失败项，始终限定为实际扫描到的 GPT 分区，不声称全设备完整备份。

目录恢复预检接受 `lun<N>/<partition>.bin` 或带显式 `--lun` 的平铺目录，排除 GPT 捕获及 `.partial`，拒绝混合布局、名称冲突、缺少所选镜像和范围溢出。XML 预检保留文件／节点顺序，列出空文件名和本地补丁跳过项；稀疏解析保留 RAW、FILL、DONT_CARE 范围，不展开整个镜像。所有选中映射和输入在首次修改前完成检查。恢复每个镜像后验证，再处理下一个；刷写先 program、再 DISK patch、再验证。RAW 和 FILL 按实际写入范围验证，DONT_CARE 不写入，也不清除前序写入的验证范围。重叠部分以后一次写入为准。

`--json` 的 stdout 只有一个最终结果对象；进度写 stderr。`--help`、`--version` 保留常规文本输出。偏移和字节数为十进制字符串，未知值为 null。退出码：0 成功，2 输入错误，3 不支持，4 执行失败，5 验证未完成，6 修改结果未知。写入确认与验证结果分别输出；传输字节计数只表示主机后端接受的 raw 数据量。

默认 `--verify best-effort` 流式比较已知内容。完整的原始数据前拒读允许继续，但最终返回 `verification_incomplete`；读取超时、截断或失步停止会话。`required` 在首次修改前拒绝 DISK 补丁和未知地址写入，运行时拒读也停止。`none` 不发送验证读取，明确标记 `not_requested`。

任何修改失败、确认缺失或回读不一致都会停止剩余修改及最终动作，不自动重试。确认缺失为 `unknown_write`；已经取得的写 ACK 不因后续读取失败而丢失。可定位 DISK 补丁仅屏蔽对应字节，位置未知的补丁屏蔽该 LUN 的验证。首版不计算补丁结果；本地文件补丁显示并跳过，不能据此证明整包兼容。

刷写的 `--set-bootable <lun>` 和 `--reboot` 只有显式请求且前序策略允许时才各执行一次；默认不重启。没有清单数据库、保存计划、哈希审批、断点续作、fastboot 或认证绕过。本次离线测试不证明 RMX3800 或其他机型受支持，也不证明固件能够启动。
