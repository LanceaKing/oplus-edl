# oplus-edl

文档导航：[项目文档](docs/README.md) · [实施进度](docs/implementation/README.md) · [评审记录](reviews/README.md) · [运行记录](runs/README.md)

通用存储能力已发布到 qdlrs fork，产品固定引用 [`205c7d4d9f15add22d78c422ba77843ab72c95d4`](https://github.com/LanceaKing/qdlrs/commit/205c7d4d9f15add22d78c422ba77843ab72c95d4)，不再使用相邻工作区路径依赖。见[发布与验证记录](docs/implementation/shared-storage-publish/README.md)。

原生 Rust Oplus EDL 命令行工具。已完成[设计](docs/designs/oplus-edl.md)的通用存储工作流与三平台离线验收。已用云端资产实刷 RMX3800 14.1.0.190 包：351 次 program 和 65 次 DISK patch 均收到 ACK。重启后出现 Android USB，用户已确认进入初始设置。ADB 已确认版本 RMX3800_14.1.0.190(CN01)，sys.boot_completed 和 dev.bootcomplete 均为 1；未完成全量回读。见[云端实刷记录](docs/implementation/rmx3800-cloud-flash-20261116.md)。

公开存储工作流包括目录备份、普通镜像／目录恢复和 XML 固件刷写。恢复和刷写先检查输入，显示摘要，确认一次后执行；非交互使用 `--yes`。`unlock`、`lock` 没有经过验证的机型方法，在连接前返回 `unsupported`。

第五阶段原生验收已完成：[GitHub Actions](https://github.com/LanceaKing/oplus-edl/actions/runs/43608522774) 的 Windows x86_64 MSVC、macOS arm64、Linux x86_64 GNU 格式检查、Clippy、构建和离线测试全部通过。Windows 为 86 个常规测试，macOS／Linux 各 88 个，另均通过 JSON 协议子进程检查；详见[第五阶段记录](docs/implementation/phase-5.md)。此前交叉编译证据单独保留，未进行设备验证。

## 构建与离线测试

```sh
cargo build --workspace --all-features --locked
cargo test --workspace --all-features --locked
```

Cargo.toml 和 Cargo.lock 固定 qdlrs 完整 Git revision `205c7d4d9f15add22d78c422ba77843ab72c95d4`。首次构建需获取该 Git 依赖，已有缓存可使用 `--offline`；不需要相邻 qdlrs 工作区。旧 revision `a80cd1e8cd1cd60634b9c5ce9a4c8c96b727c003` 不包含新的 `qdl::storage` 接口。

第五阶段产品代码已提交并推送至本公开仓库。固定依赖的历史发布及来源核验见[发布记录](docs/implementation/phase-2-publish/README.md)。已有缓存时可增加 `--offline`，首次获取依赖需要网络。验证范围见[测试记录](docs/testing.md)。

## 命令形式

下列命令是使用示例，本次实施没有运行它们。加载器由用户提供。

```sh
# 不传 --device 时仅枚举候选，返回 input_error，不上传加载器。
oplus-edl backup --output backups/session --backend usb --json

# 当前支持：选择分区；可重复 --partition，省略时备份该 LUN 全部已列出分区。
oplus-edl backup --device '<candidate>' --backend usb --loader loader.elf \
  --memory ufs --lun 0 --partition boot --output backups/session

# 恢复需明确 --device；USB 刷写可自动选择唯一 EDL 设备。两者均需 loader。
oplus-edl restore --from backups/session --verify best-effort
oplus-edl restore --partition boot --image boot.bin --lun 0
oplus-edl flash --program rawprogram0.xml --patch patch0.xml
oplus-edl unlock
oplus-edl lock
```

`flash` 完成输入预检后等待 EDL 设备连接，再打开会话、上传 loader 并初始化 Firehose。默认持续等待，按 Ctrl+C 可取消；`--wait-timeout 30` 将设备等待时间限制为 30 秒，默认值 0 表示不限时。等待提示写入 stderr，不混入 JSON stdout。USB 后端省略 `--device` 时只自动选择唯一 EDL 候选；多个候选时报错并列出候选。指定 `--device` 时只等待该目标，不会改选其他设备。串口枚举包含其他用途的端口，因此 `--backend serial` 必须指定 `--device`。设备等待只用于首次连接，刷写中断后不会自动重连或重放写入。`--prepare-only` 和 `--cloud-fetch-only` 不等待设备。

公共参数支持 `--backend usb|serial`、`--device`、`--loader`、`--signed-digest`、`--memory emmc|ufs`、`--lun`、`--sector-size` 和 `--json`。默认使用 USB、UFS 和 4096 字节扇区；eMMC 默认 512。摘要说明扇区值是介质默认值还是用户指定值，不把默认值称为实测值。当前 GPT 路径只接受 512 或 4096，配置响应给出已识别扇区字段时检查冲突。

加载器要求先接收签名分区摘要表时，可显式传入 `--signed-digest <file>`。程序在连接前读取该文件，接受 1 字节至 16 MiB 的普通文件；Sahara 和启动日志阶段结束后，按原始字节发送一次，取得完整 ACK 并确认未进入 raw 模式后才发送 configure。NAK、超时和传输错误均停止会话，不自动重试或重启。摘要表 ACK 不等于设备会话授权，也不等于读写权限；本参数不生成或提交 OSIG 会话签名，不实现链式数据包摘要。必须选择与加载器、固件及 NV 变体对应的文件。

实验性 preview 对齐新增 `--digest-signature <file>`（要求 `--signed-digest`），使用配套云端资产的 digest → 表 ACK → verify → rawmode ACK → signature → USB ZLP → ACK → sha256init → ACK 流程。可通过下文的 `--cloud-config` 自动获取配套资产；握手实现本身不证明资产适配。`flash --raw-size file` 可显式按文件长度确定 RAW 写入范围，仅补齐最后扇区并以 XML 范围为上限；默认 `declared` 保留严格检查。目标 RMX3800 包现已实现基于参考 LP 元数据的 super 合并和成功日志对应的条目选择；实际云端资产已完成下载校验，设备写入仍待验收，详见[对齐记录](docs/research/preview-rmx3800-alignment.md)。

SFP 包可使用 `flash --package`。输入必须是已经解压、包含 `IMAGES` 和 `META` 的目录。必须明确选择 NV ID，并提供一个不存在的输出目录。

```sh
oplus-edl flash --package /path/to/extracted-firmware \
  --nv-id 10010111 --prepare-output /path/to/new-output \
  --flash-config /path/to/flash_config.json --prepare-only --json
```

`--prepare-only` 生成稀疏 `super.img`、rawprogram0–4 和 `selection.json`，并运行写入输入预检，不连接设备。合并器校验包内 LP geometry、header、tables 的 SHA-256，以及分区大小、组容量和 extent 范围；保留两份 geometry 和全部主／备 metadata slot。当前仅支持单 block device、每个非空分区一个 linear extent，遇到其他布局报错。

这个 package 模式固定保护 LUN5 和 OCDT，缺失的可选 secdata 会记录为跳过。其他非空缺失输入报错。每个 LUN 保持普通条目原序，将 PrimaryGPT 放在开头、BackupGPT 放在末尾。零声明长度的 sparse userdata 按镜像展开长度处理；super 必须唯一、非零且形成与声明跨度一致的可执行条目。`--flash-config` 只读取 `excludedPartitionsForWrite`；不执行其中的擦除策略等其他设置，读取排除项不会变成写入排除项。DISK patch 与显式排除范围重叠或无法证明不重叠时，准备失败。所有筛选决定都写入 `selection.json`。

package 从 `META/build.json` 的 `ofp.product` 读取预期项目，并核对可选的 `PRODUCT_VARIANT`。首次写入前，程序只读校验六个 UFS LUN 的 GPT，定位唯一 OCDT，解析并比较设备项目号，同时检查计划范围未超过设备 GPT 所描述的容量、未触及实际 OCDT。无法读取、格式异常或不匹配均停止；该检查不受 preview 配置中的关闭选项影响。NV ID 仍由用户明确选择，没有新增未经证实的 NV 自动映射。此检查只接入 `--package`，普通 `--program` XML 不携带包项目身份。

实际刷写可使用相同 package 参数并去掉 `--prepare-only`，另提供明确的 `--device` 和匹配的 `--loader`、`--signed-digest`、`--digest-signature`。每次准备使用新输出目录。本包的 xbl_a 位于 UFS LUN1，设置启动 LUN 和重启需显式传入 `--set-bootable 1`、`--reboot`。手动复用 XML 时，逐个传入 `--program`，再传入原包 patch0–4，并使用 `--raw-size file`；此方式不执行 package 项目匹配检查。这组云端资产已完成此前实机写入；本次新增安全检查仅完成离线验证。

云端路径新增 `--cloud-config` 和 `--cloud-output`，使用 `wreq 0.16.3`。配置格式见 [examples/cloud-config.json](examples/cloud-config.json)。配置 `swm.identity_path` 和 `swm.release_directory` 后，程序自动完成 SWM 注册、challenge 应答、会话验签和 enrollment ticket 获取，无需手动票据。发布目录必须是完整、未修改的 4.2026.009 preview 解压目录。程序先验证发布清单的根签名、发布签名和文件哈希；首次运行生成本客户端的持久身份，后续复用同一身份。身份文件包含私钥，不要分享；Unix 上新文件权限为 0600。票据、会话和临时云端凭据只保存在内存。旧的手动票据／凭据配置仍兼容，但不能与 `swm` 同时使用。

查询使用 Sahara 读取的 PK hash 和 MSM ID，当前由配置提供；不要把固件项目号 `23631` 当作云端 `device_code`。自动匹配时 `device_code` 留空，按设备返回的 hash 填写前 32／48 字节的十六进制值；不足相应长度的字段留空。这些值不是对 loader 文件重新计算的摘要。

```sh
# 仅获取并校验云端资产，不打开设备。
oplus-edl flash --cloud-config /path/to/cloud-config.json \
  --cloud-output /path/to/new-assets --cloud-fetch-only --json

# 连接明确指定的设备，准备包并执行实际写入；运行时显示确认摘要。
oplus-edl flash --device '<candidate>' \
  --cloud-config /path/to/cloud-config.json --cloud-output /path/to/new-assets \
  --package /path/to/extracted-firmware --nv-id 10010111 \
  --prepare-output /path/to/new-prepared --set-bootable 1 --reboot
```

云端模块依次执行 enroll、签名 list、签名 ticket 和 HTTPS 资产下载。它验证 ticket 的 HMAC、client/profile/asset/type 绑定、到期时间、URL SHA-256、文件大小和 SHA-256。多匹配时必须设置 `profile_id`，不会选择第一个结果。全部资产通过检查后才交给 Sahara／DIGEST 初始化。临时凭据和下载 URL 不写入资产记录，跨站下载不携带 API 凭据；HTTP 重定向被拒绝。`assets.json` 记录查询、资产 ID、大小与 SHA-256。

SWM 使用固定根公钥验证服务端时间和在线密钥清单，以 ES256 签署 DPoP，并以 X25519／AES-GCM 加密请求正文。响应必须通过 `authz_v3` 的身份、nonce、期限、原始正文 SHA-256 和 Ed25519 签名检查。注册 challenge 最多应答一次；HTTP 429 或授权拒绝会停止，不更换身份重试。当前已用 RMX3800 实机 Sahara 公钥哈希，通过真实服务完成注册、自动取票、云端 enroll、匹配和三项资产下载校验。profile 11 的 loader／digest／sign 分别为 1,549,988／33,144／4,096 字节，与 Windows 成功日志中的大小一致；大小一致不证明与 Windows 缓存字节完全相同；本仓库已完成本次限定组合的设备写入。Windows／Linux 分支尚未进行原生实测。

使用配套 DIGEST signature 时，`flash` 在写入前执行 preview 的只读 `RwMode` 探测（LUN0、LBA31、5 个扇区）。两次 ACK 和完整数据通过后使用 `oplus_gptbackup` 兼容传输参数；目标 LUN、起始扇区、长度及报告中的真实分区名不变。探测失败即停止，不在未知模式下写入，也不在传输失败后自动重试。此模式来自 preview 原生参数选择及 fallback 实现，并已在本次 RMX3800 会话验证。

先启动 `flash` 并等它显示等待 EDL，再在另一终端独立执行 `adb -s <serial> reboot edl`。`flash` 不调用 ADB。

USB 后端需要主机允许访问目标接口；Windows 的 USB 接口需要适用的 WinUSB 驱动。串口后端使用系统提供的端口路径和对应串口驱动。Linux 需要相应 USB 设备或串口权限，macOS 需要允许该 USB 配件且接口未被其他进程占用。Android 的 ADB 身份与 EDL 的 9008 身份可能需要分别获得配件连接许可。实机后端验证目前仅覆盖上述 RMX3800／macOS 组合的加载器和摘要表阶段。

## 输出与限制

目录备份保存 `lun<N>/<partition>.bin`，GPT 原始捕获保存到 `lun<N>/.gpt/primary.bin` 和 `backup.bin`。先写 `.partial`，完整数据和最终 ACK 成功后才发布正式文件。失败保留 `.partial`；不自动续作。已有正式文件需显式 `--overwrite`；已有 `.partial` 即使带此选项也不截断，需选择新目录或由用户处理旧文件。

分区名必须满足设计的可移植文件名规则。未找到或同 LUN 精确重名的分区不会被猜测匹配。单项精确选择不受未选中大小写相似名称影响，已有目标文件仍受覆盖规则约束。主 GPT CRC、条目 CRC、范围和重叠会检查。主 GPT 无效时本阶段报错，不自动从备份 GPT 修复。主 GPT 和备份 GPT 捕获跨度分别限制为 16 MiB。备份 GPT 保存实际读取的字节，不声称它与主 GPT 一致。

主 GPT 探测和完整读取分别记录结果。探测失败时 partial 仅包含从 LBA 1 读取的字节，起点见结果项。默认文件发布采用系统独占重命名，不要求硬链接；若系统不支持该操作，则保留 partial 并报错，不降级为可能覆盖已有目标的操作。

省略 `--lun` 时查询存储信息，识别明确的物理单元数量并扫描这些 LUN。无法识别数量时返回不支持，可显式指定 `--lun`；不通过读失败推断扫描结束。当前识别响应属性 `num_physical_partitions`、`num_physical`、`bNumberLu` 和日志 JSON 的 `storage_info.num_physical`。扫描或读取失败会停止会话；结果保留完成项和失败项，始终限定为实际扫描到的 GPT 分区，不声称全设备完整备份。

目录恢复预检接受 `lun<N>/<partition>.bin` 或带显式 `--lun` 的平铺目录，排除 GPT 捕获及 `.partial`，拒绝混合布局、名称冲突、缺少所选镜像和范围溢出。XML 预检保留文件／节点顺序，列出空文件名和本地补丁跳过项；稀疏解析保留 RAW、FILL、DONT_CARE 范围，不展开整个镜像。所有选中映射和输入在首次修改前完成检查。恢复每个镜像后验证，再处理下一个；刷写先 program、再 DISK patch、再验证。RAW 和 FILL 按实际写入范围验证，DONT_CARE 不写入，也不清除前序写入的验证范围。重叠部分以后一次写入为准。

`--json` 的 stdout 只有一个最终结果对象；进度写 stderr。`--help`、`--version` 保留常规文本输出。偏移和字节数为十进制字符串，未知值为 null。退出码：0 成功，2 输入错误，3 不支持，4 执行失败，5 验证未完成，6 修改结果未知。写入确认与验证结果分别输出；传输字节计数只表示主机后端接受的 raw 数据量。USB raw 写入失败且适配器未提供完成长度时，`transferred_bytes` 为 null，不能将其当作零字节写入。

默认 `--verify best-effort` 流式比较已知内容。完整的原始数据前拒读允许继续，但最终返回 `verification_incomplete`；读取超时、截断或失步停止会话。`required` 在首次修改前拒绝 DISK 补丁和未知地址写入，运行时拒读也停止。`none` 不发送验证读取，明确标记 `not_requested`。

任何修改失败、确认缺失或回读不一致都会停止剩余修改及最终动作，不自动重试。确认缺失为 `unknown_write`；已经取得的写 ACK 不因后续读取失败而丢失。可定位 DISK 补丁仅屏蔽对应字节，位置未知的补丁屏蔽该 LUN 的验证。首版不计算补丁结果；本地文件补丁显示并跳过，不能据此证明整包兼容。

刷写的 `--set-bootable <lun>` 和 `--reboot` 只有显式请求且前序策略允许时才各执行一次；默认不重启。没有清单数据库、保存计划、哈希审批、断点续作、fastboot 或认证绕过。离线测试不证明其他 loader、固件或机型组合可用；本次实机结果仅适用于记录中的输入。

preview 刷写指定启动 LUN 时，程序会在首次写入前读取该 LUN 的 UFS Boot LUN ID；ID 为 0 或无法确认时停止。物理 LUN 编号与 Boot LUN ID 不是同一字段。
