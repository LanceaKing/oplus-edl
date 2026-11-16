# RMX3800 与 preview 云端 loader 路径对齐

> 后续进展：已使用云端配套资产完成指定 RMX3800 固件的写入及启动核对，全量回读仍未完成。本文的“部分实现”等状态保留为当次记录。详见[实刷记录](../implementation/rmx3800-cloud-flash-20261116.md)；总览见[研究索引](README.md)。

日期：2026-11-15（Asia/Shanghai）。状态：部分实现，尚未完成整包刷写支持。

用户确认在 Windows 上使用最新 preview 的**云端自动匹配 loader**，成功刷入 `RMX3800domestic_11_14.1.0.190CN01_2025012302300313.zip`。这是用户实测信息；当前 Mac 未取得那次会话的日志或云端资产。此前原包 programmer 的拒读结果不能用来否定这条路径，也不能用来确定云端 loader 的身份。

本轮重新查询[发布页](https://oplustool.anteasy.com/downloads)的公共 preview 列表，首个可下载版本仍为 4.2026.009 / release 48。使用现存同版本二进制，在 Binary Ninja 5.2.8614 GUI 内重新导出 HLIL 和引用关系。QCOM DLL SHA-256 为 `05562025f23535bf291a5c19ba5a4bfb792a5945a2f67526dc8eb118b8ca1ca5`。

## 已恢复并接入的行为

### DIGEST 初始化

EXE `0x1405b2bc0` 调用 `0x14057ff50`，固定两个模式参数为 `(1, 0)`，接收上限为 1 MiB。后者构造 QCOM `oplus_qcom_digest_auth_create` 的 32 字节配置。结合 QCOM `0x1800424c0` 和 `0x180042640` 的状态转换，顺序为：

1. 原样发送 digest 文件的全部字节。
2. 发送 `<?xml version="1.0"?><data><verify value="ping" EnableVip="1"/></data>`。
3. 原样发送配套 signature 文件的全部字节。
4. 等待完整响应；失败停止。
5. 发送 `<?xml version="1.0"?><data><sha256init value="ping"/></data>`。
6. 等待完整响应；失败停止，然后才进入存储配置。

该 profile 不在第 1 或第 2 步后读取 ACK。不能套用当前原包表上传的“发送表后立即等 ACK”流程。也不能把任意 256 字节签名自动补到 4096 字节：本实现只发送指定文件的原始内容，材料的格式和大小由匹配资产决定。

新增 `--digest-signature <file>`，要求同时指定 `--signed-digest <file>`。不传新参数时保留原有表上传流程。新流程验证最终 ACK 和 XML 模式；NAK、raw 模式、传输失败或超时使初始化失败。两个二进制输入都在 Sahara 前检查非空及 16 MiB 上限。此上限是产品输入限制，不是从 preview 推导的资产大小。

这仅实现协议传输，不生成厂商签名，不自动选择云端资产，不完成 SWM enrollment 或票据请求。状态消息只说明握手收到确认，不说明设备权限或固件适配成功。preview 的完整写入 DIGEST 状态、分区摘要、命令参数及实际云端 loader 仍需对齐。

### RAW 文件长度

EXE `0x1405425f0` 组装原生 RAW 计划参数；QCOM `0x180053da0` 按模式和 force 参数计算扇区数。XML 模式且 force=false 时：

```text
write_sectors = min(ceil(file_bytes / sector_size), xml_sectors)
source_bytes = min(file_bytes, write_sectors * sector_size)
zero_padding = write_sectors * sector_size - source_bytes
```

新增显式 `flash --raw-size file`，将上述规则用于 RAW 文件可用范围；有 source offset 时从该偏移计算可用长度。只补齐最后一个扇区，不把缺少的整个分区尾部补零。默认 `--raw-size declared` 保持严格 XML 范围检查。稀疏文件处理不变。force=true 的分支可以要求完整 XML 长度，但本轮没有给未知容器或标签启用该行为。

这解决普通短 RAW 镜像的预检问题，不证明短文件一定是可直接写入的 RAW 数据，不自动改名或解码包内特殊容器。

## 验证

- 原生执行：Unicorn 执行原 DLL 指令，9 组 RAW 长度案例和 4 个 DIGEST 状态转换通过。没有指令补丁、hook、密码替代函数、网络请求或设备 I/O。构造的是协议测试上下文，不是有效的设备认证上下文。
- 脚本化真实协议测试：新增完整 DIGEST 顺序、1/7/4096 字节分片、512/4096 字节原样传输、签名及 sha256init 拒绝、输入前置拒绝测试。
- 文件预检：新增 RAW 最后扇区对齐、XML 上限、source offset、空文件及默认严格模式测试。
- `cargo test --locked`：98 个常规测试及 JSON stdout 协议子进程检查通过。
- `cargo fmt`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo build --locked` 通过。本轮未执行 Windows/Linux 原生检查。

真实包六份 XML 的离线预检使用显式虚拟设备名称、未指定 loader，因此在设备枚举前退出。rawprogram1/2 已越过镜像预检，到达“需要 loader”；rawprogram0/3/4/5 分别停在 `super.img`、`oplus`、`sec.dat`、`dynamic_nvbk.bin` 缺失。没有把这些文件静默跳过。

## 未完成的验收条件

1. 取得 Windows 成功会话实际使用的 loader，以及存在时的 digest/signature；记录它们的大小、SHA-256、scheme 和匹配信息。现有发布 ZIP 不是云端资产。
2. 根据成功日志或实际资产确定是否使用本轮恢复的 DIGEST 分支；云端自动匹配本身不能证明 scheme 必然为 DIGEST。
3. 对齐 super 构建、NV 变体、短容器、别名、被跳过的分区、写入摘要与命令参数。不能把原包的 `prog_firehose_ddr.elf` 配上未知云端签名，也不能把现有别名文件直接补零写入。
4. 用相同材料验证 Sahara、握手、存储权限，再进行整包写入、回读和启动验收。

当前不能给出已验证的整包刷写命令。本轮未连接手机、未发送分区写入、未提交或推送。

证据见 [目录](preview-rmx3800-alignment-evidence/manifest.json)。测试及构建日志位于 `runs/preview-rmx3800-alignment/`。原生验证脚本按仓库根路径解析样本，应从该 runs 目录运行；证据目录保留同一脚本的归档副本。


## Windows 成功会话日志补充（2026-11-16）

用户提供 `session_20261115_221424_126_109520.zip`，本地解析记录见 `runs/preview-rmx3800-windows-success/summary.json`。以下结论来自原始日志，不是本仓库的实机结果。前文“未取得成功会话日志”和“scheme 待确认”现已被本节补齐；云端资产字节仍未取得。

- 自动匹配得到 `SM8650 (DIGEST)`；MSM ID 为 `0x002270E1`，HWID 为 `0x002270E100510000`，Sahara 身份查询为 V3。
- 三轮下载均依次校验 1,549,988、33,144、4,096 字节。结合此前下载调用顺序，对应 loader、digest、sign；日志大小行自身没有文件名或哈希。包内 programmer 为 1,558,188 字节，因此与该云端 loader 不是同一个完整文件。
- 三轮均记录 digest → verify → sign 响应 → sha256init 响应 → DIGEST 授权成功，随后发送启动命令并配置设备。启动命令的原始 XML 未记录，仍须对齐，不能把现有握手实现等同于完整初始化。
- 成功读取 145 个分区。刷写前回读 GPT/OCDT，项目号 23631 校验通过。
- 明确保护 LUN5，跳过 rawprogram5.xml；选择五个 rawprogram 和 patch0–4。不能以本记录证明 LUN5/NV 写入已验证。
- 最初没有 super.img，后续在 22:27:40 检测到已合并的 super.img，并按标准整包写入模式处理。日志没有记录合并动作；用户随后明确确认 super.img 是工具自动合并的。因此需要复现工具的自动合并流程，不能将预先提供 super.img 当成最终支持条件。具体合并参数仍须从代码恢复。
- 共记录 57 个刷写条目；包含 super、userdata，未列出 ocdt 或 secdata 的写入。OCDT 读取校验不等于写入。
- 写入顺序中各 LUN 的 PrimaryGPT 在前、BackupGPT 在后；随后应用五个 patch 文件并设置启动分区。22:34:38.564 记录“刷写操作完成”，session.json 最终状态为 Succeeded。

新发现的实现差异：目标原包 userdata XML 的零扇区条目在本仓库仍被跳过，但该成功会话实际记录 userdata 写入；不能把零扇区一概视为空操作用于本包适配。还须对齐 GPT 排序、所选条目、启动命令及启动分区参数。日志没有实际 program 长度，不能仅凭完成行验证 RAW 长度策略。

此 ZIP 没有 loader/digest/sign 本体、资产 SHA-256、刷后回读或 Android 启动记录，也没有目标 ZIP 的完整路径/哈希。它证明客户端报告上述选择下刷写完成；结合用户说明关联目标包，但不独立证明每个镜像均来自该 ZIP 或刷后启动成功。

下一份有用材料是当时的 `flash_config.json`、已合并 super.img 的来源/大小/哈希及实际云端资产。配置快照若在会话后改过，须明确不是当时状态。


### 用户提供的刷写配置

配置原件保存于 `runs/preview-rmx3800-windows-success/flash_config.json`。

- `excludedPartitionsForRead=["userdata"]`：读取排除列表含 userdata；不能据此判定写入或内部回读行为。
- `excludedPartitionsForWrite=[]`、`excludedPartitionsForErase=[]`：两个配置排除列表均为空。这不代表界面选中全部分区，也不覆盖日志中独立的 LUN5 保护开关。
- `validateQualcommOcdtProjectBeforeFlash=true`：与成功日志中的 23631 项目校验一致。
- `partitionEraseMethod=1`：仅记录枚举值；尚未恢复对应语义，不能推断为整盘擦除。
- `validateMediatekV5OfpOcdtBeforeFlash=false`：属于 MediaTek 配置，不用于本次 Qualcomm 路径。
- `version=1.0` 是配置字段，不是 OplusTool preview 的版本号。

配置文件没有云端资产路径、super 合并参数、NV 选择或 LUN5 保护字段。该文件未携带可证实的快照时间；用户没有另外确认刷机后是否修改过，不能用它独立证明全部历史设置。它也不能解释未写入 ocdt/secdata 的原因，需要追踪实际计划生成和条目选择。


## 2026-11-15: implemented package super and selection

本次继续使用 Binary Ninja 5.2.8614 导出当前 preview 控制流。导出存放在 `runs/preview-super-implementation/`。App `0x140741b80` 查找现有 `super.img`，`0x1407440d0` 将该文件绑定到普通整包刷写分支并输出成功日志中的兼容提示。`0x14071de60` 绑定并选中合并镜像。这个提示本身不能确定文件是谁生成的；本次用户补充确认工具自动合并。

Oplus_Image.dll `0x18002eb40` 进入 reference-layout 分支 `0x1800094e0`，后者要求声明全部 package metadata groups。`0x18002f020` / `0x18000a950` 导出 metadata region，`0x18000c540` 分配 `0x3000 + 2 * metadata_max_size * slot_count` 字节，写两份 geometry 和所有主／备 metadata。包内 `super_meta.10010111.raw` 的开头 `67446c61` 是标准 LP geometry magic，不是加密容器。它给出 65536-byte metadata slots、3 slots、4096-byte logical block，header 在 offset 4096。

产品新增 `src/super_image.rs`，使用包内已校验 reference metadata 和定义映射，将 17 个输入镜像的 sparse RAW/FILL/DONT_CARE 段移入目标 extent。它保留 reference bytes，没有声称重现 preview 构建器的全部布局重排能力。`src/package.rs` 将成功日志对应的选择策略接入 flash，生成可复查的 selection.json。57 项的文件名、label 和顺序逐项匹配本次成功日志；没有硬编码 57 项清单。缺失的普通输入仍报错，仅已观察到的可选 secdata 缺失可跳过。NV ID 由用户显式选择，不猜测设备变体。

实际目标包离线结果见 `runs/preview-super-implementation/validation.json`。生成 super 的展开长度为 15569256448 字节，共 318 个 sparse chunks。独立 Python 检查两份 geometry、六份 metadata，逐段比对 17 个源镜像和输出对应区段的 payload SHA-256、类型、长度、偏移，全部一致。该比较证明合并输出对应包内参考布局和输入内容，不证明与 Windows 未导出的 super.img 字节一致。

格式依据为 AOSP Android 14 的 [LP metadata format](https://android.googlesource.com/platform/system/core/+/refs/heads/android14-release/fs_mgr/liblp/include/liblp/metadata_format.h)、[metadata writer](https://android.googlesource.com/platform/system/core/+/refs/heads/android14-release/fs_mgr/liblp/writer.cpp) 和 [offset calculation](https://android.googlesource.com/platform/system/core/+/refs/heads/android14-release/fs_mgr/liblp/utility.cpp)。未执行设备写入；云端 loader/digest/sign、OCDT project 校验与刷后回读/启动仍属于独立的设备验收边界。当前 flash-config 只应用 excludedPartitionsForWrite，其他设置不执行。


## Cloud request implementation and actual program parameters

本次实现 `src/cloud.rs`，使用 wreq 0.16.3，并接入 flash 的云端资产输入。Binary Ninja 原始导出存放于 `runs/cloud-flash-implementation/`。`loader.dll!0x180056bc0` 构造 `POST\npath\nquery\nbody_sha256\ntimestamp_seconds\nnonce_hex\nclient_id`，空 query 保留空行；HMAC-SHA256 key 是 enrollment 返回的 client_secret 原始字符串。`0x180058da0` 使用小写十六进制，版本字符串在 `0x1801595d0` 为 `1`，nonce 是 16 字节随机值。`0x18003ebb0` 解析 client_id、client_secret、delegation_id、expires_at；正常 enrollment 在 `0x18003f3b0` 校验过期时间及最长 605 秒窗口。

`0x180043f90` 明确 ticket 字段为 success、download_url、download_url_sha256、file_type、checksum_sha256、size、manifest_version、client_id、profile_id、asset_id、expires_at_unix、manifest_signature。`0x180044cf0` 构造带字段名的 cloud_loader_ticket_v2 manifest 并验证 HMAC；`0x180044c40` 限制普通资产为 64 MiB。实现保留绑定、过期、摘要与 HMAC 检查，拒绝跨 profile/type 替换。云端 acquisition 不能绕过正常 SWM enrollment ticket 前置条件。

`src/firehose.rs` 修复原固定 qdlrs program API 只用 label 显示进度、未放入 XML 的缺口。产品现在发送经 XML 转义的 label，保留 USB 短写不重发、传输失败停止、最终 ACK 确认和 unknown_write 语义，并检查显式 rawmode。脚本协议测试覆盖 Sahara → digest → verify → sign → sha256init → configure → program → patch → setbootablestoragedrive → power 的同一会话。

wreq 真实匿名 list 探测返回 401，记录见 `runs/cloud-flash-implementation/live-wreq.log`。本地 HTTP 服务测试验证 enroll、list、ticket 的真实请求字节与 HMAC，并确认错误 manifest 不触发下载或发布。HMAC 固定向量与独立 Python hmac 结果一致。测试不证明服务端接受有效 SWM 会话，也不证明当前云端有可用的匹配资产。当前未枚举到本机 EDL 设备，未执行物理刷写。


## 云端 loader 实机握手修正（2026-11-16）

已取得 profile 11 的真实云端资产，哈希见 `swm-live-cloud-assets.json`。两次实际连接均完成 Sahara 上传，但在 DIGEST 的 verify／sign USB OUT 阶段超时取消；尚未执行 program。第一次记录在 `runs/live-cloud-flash`，第二次记录在 `runs/live-cloud-flash-attempt2`。

从云端 loader 文件偏移 `0x10c154` 提取 AArch64 ELF 作 Binary Ninja 分析，未改动发送给手机的文件。`0x149a4320` 接收并验证表，成功后调用 `0x149a42c4(1)`；后者以 `0x149a7780` 生成 ACK，经 `0x149a461c` 阻塞发送，再接收命令。因此上文依据主机 DLL 推导的“没有表 ACK”不适用于同步 USB 实现。产品已改为 digest → table ACK → verify → sign → signature ACK → sha256init → ACK。表拒绝或 rawmode 异常立即停止。

产品同时在收到 `INFO: VIP is enabled, receiving the partition info of size ...` 时结束启动日志读取，避免额外等待 USB 超时。36,864 是 loader 接收缓冲区大小；继续原样发送下载的 33,144 字节 digest，不据此补零。

第三次实机已通过 table ACK，随后在 sign 发送时阻塞。继续恢复云端 loader 命令表：verify 位于 `0x149a185c`，先经 `0x149a1ac4 → 0x149a7fb8(1,1)` 返回 `ACK rawmode=true`，再由 `0x149a1218` 接收 4096 字节 sign。因此完整同步顺序还包含 verify 后、sign 前的 rawmode ACK；产品已补齐并检查 rawmode。原主机状态机省略的响应不能照搬到同步传输实现。

第五次加入 sign USB ZLP 后，完整握手和存储配置通过，前 7 项 program 获得 ACK，super 的首个 program 被 NAK（零数据传输）。设备日志确认签名表的 super 范围起点为 361196，目标包为 99052，直接标签受范围限制。

继续恢复原工具 RwMode：`0x180035c60` 的 kind 1 使用 LUN0/LBA31/5 扇区，filename `gpt_backup0.bin`、label `BackupGPT`、special_rw_mode `oplus_gptbackup`。本次只读探测得到两次 ACK 和 20480 字节。`0x18005c600 → 0x18003ec20` 对该模式选择参数 10；`0x18005b940 → 0x18003df70` 的原生 fallback 构造当前 LUN 的 gpt_backup 文件名和 BackupGPT 标签。产品加入该预先探测的传输模式，保留实际物理范围与报告分区身份；未改动签名表。恢复任务明确跳过前 7 个已 ACK 条目，从 super 继续，未执行自动断线重放。
