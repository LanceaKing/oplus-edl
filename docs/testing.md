# 测试状态

## 当前工作区

GUI 与 CLI 等价回归通过真实适配器运行脚本传输，比较协议命令、字节计数、项目状态和终态。云端用例只连接本机环回服务，验证无效票据 HMAC 被拒绝。[检查点 C 记录](implementation/desktop-migration.md#checkpoint-c-real-desktop-adapter)

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings
cargo build --workspace --all-features --locked --offline
cargo test --workspace --all-features --locked --offline
cargo test -p oplus-edl-core --no-default-features --test public_api --locked --offline
cargo test -p oplus-edl-core --no-default-features --features test-support --test session --locked --offline
cargo test -p oplus-edl-core --no-default-features --test package --locked --offline
cargo test -p oplus-edl-core --no-default-features --features cloud --lib --locked --offline
cargo test -p oplus-edl-core --no-default-features --features cloud,test-support --test service --locked --offline
cargo test -p oplus-edl-desktop --no-default-features --lib --locked --offline
cargo test -p oplus-edl-desktop --no-default-features --test app --test frontend_parity --locked --offline
cargo test -p oplus-edl-desktop --no-default-features --example native-qualification --locked --offline
```

## 停止边界审计

依据锁定的 qdl `9161c0f46cef245f3b5fc5394dba1ec5e8c3f6ac`、核心 worker 和实际前端入口核对超时与停止边界。离线回归使用脚本传输、注入的打开 provider 和本地临时文件。原生对照及 USB 在途读取计时待实测。

| 路径 | 超时与停止边界 | 离线证据和原生验收 |
| --- | --- | --- |
| 设备打开 | 发现等待每 250 毫秒检查停止。设备 worker 在调用打开 provider 前检查停止。USB 在枚举后同步等待设备 open。串口同步打开并应用端口设置。两者在打开成功后才设置传输超时。已进入的打开调用由平台返回。返回后在首条初始化交换前检查停止和材料。 | 在脚本 provider 的打开调用中请求停止。核对调用返回前任务继续等待，返回后丢弃通道且无初始化。另用被阻塞的旧通道释放验证停止后无替换打开。原生授权夹具记录正常打开、访问拒绝或被占用时的进入和退出。 |
| USB 接口 claim | qdl 在打开后等待 `detach_and_claim_interface` 返回，再配置端点读超时。调用中的停止由外层 worker 接收，等待 claim 返回后在初始化边界处理。 | 脚本 provider 覆盖打开返回边界。原生诊断构建对 open 和 claim 分别打点，记录接口号、操作系统的返回状态、停止请求和后续协议记录。 |
| 应答循环和 raw 传输 | 串口读写和 USB 的 bulk 读写使用各自的十秒 I/O 超时。Firehose XML 的十秒响应宽限期从首次停止起算。核心在协议应答边界检查宽限期，实际 USB refill 返回后处理。raw 命令按事务边界保留已发送字节、ACK 和结果。一次命令可包含多次底层 I/O。 | 脚本回归覆盖持续日志、部分 XML、读写超时、及时 ACK 及无后续命令。按下节原生规程测量在途读。在授权只读夹具上记录读写传输的阻塞进入、返回结果及可用的取消完成事件。 |
| 宿主文件系统 | 解析路径、打开、单次读取、同步和发布使用宿主的同步调用。开始下一份 XML、下一普通源或包 patch 读取前检查停止。Sparse 元数据读取在头部及各 chunk 边界检查停止。SWM 发布成员必须是普通文件，逐个成员打开前及每次最多 64 KiB 哈希读取前检查停止。复核身份路径返回后，加载或排他创建身份前再次检查。已开始的系统调用由宿主返回。 | 脚本与临时文件回归分别覆盖普通 XML、包和 sparse 解析中停止，SWM 验证和身份访问边界，以及备份读取前的输出文件检查。原生验收在临时普通文件、受限输入或慢卷上记录实际调用、返回和终态，核对已完成工作以及后续调用边界。 |

每次原生对照用同一候选和可复核的探针，保存宿主、后端或卷类型、归档 SHA-256、各调用的进入／返回、首次停止、终态与相应的命令或文件记录。设备侧只执行授权的发现、打开或只读事务。记录仍在等待的系统调用和采集窗口，按实际边界判断停止行为。[核心会话回归](../crates/oplus-edl-core/tests/session.rs) · [工作槽回归](../crates/oplus-edl-core/tests/service.rs) · [Sparse 回归](../crates/oplus-edl-core/src/sparse.rs) · [SWM 回归](../crates/oplus-edl-core/src/swm.rs)

## 检查点 D 发布包

按[开发指南](development.md#生成发布包)使用显式生产特性独立构建。脚本检查两套入口、目标架构、平台元数据、许可文本和逐文件哈希，并打印归档 SHA-256。本机 macOS arm64 已通过离线打包、解包核验和 CLI 版本／帮助检查。[检查点 D 记录](implementation/desktop-migration.md#checkpoint-d-release-packaging)

### 解包后的生产程序

在未安装 Rust 的 Windows 11 x86_64、macOS 14 arm64 和 Ubuntu 24.04 x86_64 GNOME 主机，以普通用户启动解包后的程序。Ubuntu 分别在 X11 和 Wayland 验收。核对 1280 × 800 与 1024 × 768 下的工作区、中文输入法、文件／目录选择与取消、空设备列表、USB／串口选择、缺失输入、已有 `.partial` 和未配置云端时的离线工作。访问拒绝用例使用相应的受限后端或输入权限，记录实际设置与结果。[设计的验收要求](designs/oplus-edl-desktop.md#7-package-document-and-validate-the-selected-release-baseline)

无设备时，用以下步骤复现等待中的停止与关窗。

1. 断开受测主机上的 EDL 和串口设备。在新建的临时目录写入一个仅包含 ASCII `abcd` 四字节的 `fixture.bin`。在 Unix 可用 `d=$(mktemp -d); printf abcd > "$d/fixture.bin"`。在 Windows PowerShell 可用 `$d = New-Item -ItemType Directory -Path (Join-Path ([IO.Path]::GetTempPath()) ([Guid]::NewGuid())); [IO.File]::WriteAllBytes((Join-Path $d.FullName 'fixture.bin'), [Text.Encoding]::ASCII.GetBytes('abcd'))`。
2. 打开解包后的 GUI。选择 USB，设备键填 `qualification-only`，加载器选择该临时文件。启用「等待设备发现」，时长填 `30` 秒。点击「连接」。等待状态显示后点击「停止」。用录屏分别记录点击时刻、「已请求停止」首次可见时刻，以及「已停止」终态和未连接状态。请求反馈与终态可以出现在同一帧。快速完成时，底部仍提供该设备任务的停止反馈，开始下一设备任务时该条反馈退出。
3. 再次点击「连接」。在相同等待状态下正常关闭窗口，记录关闭请求及进程退出。保留相应截图或录屏和主机、归档哈希。此场景只经过设备等待。

在同一显示器上以 100%、110% 和 200% 应用缩放分别调整窗口，使用系统的逻辑客户区尺寸记录调整后的宽高。按 Ctrl+0 重置应用缩放。每次按 Ctrl+= 增加 10 个百分点，macOS 使用 Command 代替 Ctrl。正常关闭后重新打开，核对顶部缩放值和客户区宽高分别恢复。分别在 1280 × 800 与 1024 × 768 执行，包含 Ubuntu 的 X11 和 Wayland。记录系统显示缩放、应用缩放和窗口管理器对客户区尺寸的实际限制。

#### 已有分区部分文件

使用另行授权的只读验收夹具。夹具须能完成初始化，在 LUN 0 提供有效的主、备 GPT，并有名为 `boot` 的普通分区。夹具须能导出从宿主接收的 Firehose XML 命令。记录夹具、加载器和签名材料的版本。此项在解包后的生产 GUI 执行。

1. 在新的临时备份目录创建 `lun0/boot.bin.partial`，内容为 ASCII `partial-sentinel`，无换行。Unix 可执行 `backup=$(mktemp -d); mkdir "$backup/lun0"; printf %s partial-sentinel > "$backup/lun0/boot.bin.partial"`。Windows PowerShell 可执行 `$backup = New-Item -ItemType Directory -Path (Join-Path ([IO.Path]::GetTempPath()) ([Guid]::NewGuid())); $lun = New-Item -ItemType Directory -Path (Join-Path $backup.FullName 'lun0'); [IO.File]::WriteAllBytes((Join-Path $lun.FullName 'boot.bin.partial'), [Text.Encoding]::ASCII.GetBytes('partial-sentinel'))`。记录文件的初始字节或 SHA-256。
2. 在 GUI 连接夹具。切换到「备份」，将 LUN 填为 `0`，再点击「检查分区」。从夹具 GPT 记录 `boot` 的物理 LUN、扇区大小、起始扇区和扇区数。只选 `boot`。将「备份目录」设为临时目录，取消「允许替换已有正式文件」。
3. 分区检查结束后，清空夹具接收端的命令记录，或记录当前末尾序号。确认记录器已启用后，点击「开始备份」。GUI 显示终态后，导出这段命令记录。保留顺序和原始 XML。对每条 `read` 提取 `physical_partition_number`、`SECTOR_SIZE_IN_BYTES`、`start_sector` 和 `num_partition_sectors`。对照夹具 GPT 核对主、备 GPT 的读取范围。确认 LUN 0 的读取范围均与 `boot` 的数据范围无交集。
4. 展开右侧的「项目与回读结果」。按分区名、LUN 和字节范围定位 `boot`。核对执行状态 `not_attempted`、传输字节 0 和已有输出或部分文件的冲突提示。核对 GPT 读取的 `done` 项和字节数。按初始字节或 SHA-256 核对 `boot.bin.partial`，并检查 `boot.bin` 仍不存在。保存夹具命令记录、结果截图和 GUI 诊断导出。

### 脚本负载下的工作区交互

在对应架构的构建主机获取锁定依赖后，独立构建资格测试程序。macOS 构建时设置 `MACOSX_DEPLOYMENT_TARGET=14.0`。

```sh
cargo build -p oplus-edl-desktop --example native-qualification --no-default-features --release --locked --offline
```

在同一验收基线启动 `target/release/examples/native-qualification`，Windows 使用 `.exe`。程序加载 10,000 条合成 GPT 记录。待表格出现后，点击「开始 30 秒离线准备」。设备槽空闲，离线槽延迟执行本地准备。点击第一列「选中」下的复选框，记录已选计数变化。滚动并记录可见的分区行变化。点击底部离线槽的「停止」，记录「已请求停止」及随后终态。用录屏帧测量三项输入到对应变化的时间，阈值均为 250 毫秒。在 100%、150%、200% 缩放和两种窗口尺寸下重复。另跑一次到完成，检查清空日志后结果仍可读。

为每次验收保存候选提交、CI 运行链接、系统版本与架构、显示会话、缩放、普通用户身份、实际归档或资格程序 SHA-256、测试结果和测量证据。分别记录生产包与资格程序对应的测量。本机工作树归档使用评审输入指纹和本地日志；固定提交的 CI 归档使用实际 CI 运行链接和归档摘要。设备操作、USB 在途读取计时及上述原生验收待实测。实机记录还需包含传输、加载器、固件、执行操作和实际结果。

### USB 在途读取计时

本项待实测。使用另行授权的 USB 只读验收夹具。夹具须能用选定加载器完成初始化、为单个 LUN 返回有效的 GPT 头，并能延迟该读取的 Firehose XML 终止 ACK，或用不终止应答的日志保持会话。保存夹具版本与响应脚本。使用候选锁定的 qdl 和原生 USB 后端，记录后端读取超时及设备接口。命令只请求分区检查。将下列 `KEY`、`LOADER`、`MEMORY` 和 LUN 替换为夹具已验证的参数，根据已验证配置提供相应签名材料。

```sh
oplus-edl partitions --backend usb --device KEY --loader LOADER --memory MEMORY --lun 0 --json -q
```

1. 从同一候选制作计时诊断构建。使用发布打包的锁定依赖和生产特性，保留候选及 qdl 指纹和临时探针差异。程序内共用同一单调 `Instant` 起点。可在临时 qdl 源中用 `OnceLock<Instant>` 提供打点函数，让核心和 qdl 写入同一缓冲记录。终态交付后导出缓冲记录。
2. 在 `JobHandle::request_stop_with_reason` 首次写入 `StopRequest.at` 时记录 `t_stop`。在 qdl `QdlUsbConfig::fill_response_buf` 的 USB refill 中，记录 `packet.read` 和适用的短包收尾的 `t_read_start`、`t_read_return`、读序号及 I/O 结果。用 USB 提交记录或夹具侧证据确认相应 bulk IN 请求已在途。GPT 头已传输且终止应答读在途后，在 CLI 控制台按 Ctrl+C。保留满足 `t_read_start < t_stop < t_read_return` 且原生请求在 `t_stop` 时仍在途的样本。
3. 将 `Job::finish` 锁内写入 `job.result` 的时刻记为 `t_core_result`。将设备 worker 完成丢弃会话通道的时刻记为 `t_worker_drop`。失败关闭在 `Job::finish` 的 `drop(channel)` 返回后打点。CLI 关闭可复用会话时，在 `device_worker` 处理 `Message::Close`、丢弃通道后、回复 `close_idle()` 前打点。每个通道只记录一次丢弃事件。测量 CLI 交付时，再记录终态 JSONL 写入并 flush 成功的 `t_jsonl`。
4. 用 nusb 原生传输完成回调的探针或宿主 USB 跟踪观测底层请求。记录与该读匹配的 bulk IN 请求标识、提交事件和最终状态。将正常完成或取消完成的时刻记为 `t_native_done`。取消请求可以单独记录。如果也测量接口释放，从独立的原生事件记录 `t_interface_released`。原生跟踪使用独立时基时，保留原始事件和时基，完成对齐后再计算跨时基的时间差。观测窗口内未见完成或取消时，将该事件记为未观测。
5. 做三次对照。及时释放终止 ACK 时，核对已完成 GPT 读取的字节数和 CLI 的已停止终态。核心先提交可复用结果，随后 CLI 回收会话，满足 `t_core_result <= t_worker_drop`。原生后端早于截止超时时，核对 `io_timeout`。跨截止对照中，在停止后续发非终止日志，使同一次应答等待在十秒宽限期内进入后续 USB refill。随后暂停发送，使该读在原截止之后返回，核对 `response_wait_expired`。两种失败对照均关闭会话，要求 `t_worker_drop <= t_core_result` 和只读执行失败终态。三次均核对首次 `signal`、已完成项、JSONL 在两个程序事件之后交付，以及命令记录无后续 Firehose 命令和隐式 reset。单独记录实测的原生完成或取消顺序。
6. 记录 `t_deadline = t_stop + 10 s`。报告首次在途读的 `t_read_return - t_stop`，以及跨截止读取增加的 `max(0, t_read_return - t_deadline)`。报告 `t_core_result - t_stop`、`t_worker_drop - t_stop` 和适用的 `t_jsonl - t_stop`。原生事件已观测且时基可比时，另报 `t_native_done - t_stop` 和适用的接口释放时间。保存原始事件、读序号、USB 请求标识、命令记录、退出码和 JSONL 终态。若未观测到在途或跨截止读，记录实际情形并重试对应对照。记录平台、USB 后端、测试二进制 SHA-256 和探针版本。计时证据归属该原生诊断构建。

## 历史记录

存储分层时，通用 GPT 与传输回归迁至 fork `qdl/tests/storage.rs`，fork 的 CLI 适配由 `cli/tests/storage.rs` 覆盖。产品的适配、目录与备份结果测试现由 `oplus-edl-core` 的 `storage` 目标持有。基线 qdlrs `205c7d4d9f15add22d78c422ba77843ab72c95d4` 的 Git 依赖证据见[发布记录](implementation/shared-storage-publish/README.md)。

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

`cargo test -p oplus-edl-core --features cloud --lib swm --locked --offline` checks captured server root signatures, nonce replay rejection,
identity reuse, ES256 proof verification, and signed response raw-body binding.
The public fixtures in `crates/oplus-edl-core/tests/fixtures/swm` contain signed server time and online
key metadata only. They contain no client private keys, session tokens, or tickets.
An explicit live acquisition check uses `oplus-edl cloud fetch --config FILE --output NEW_DIRECTORY`.
Record it as an acquisition check. Keep ADB reboot outside the flash invocation.

本次 macOS 实机验证先启动主机等待，再独立执行 `adb reboot edl`，成功读取 Sahara v3 身份及 48 字节 PK hash。随后运行产品 `flash --cloud-fetch-only`，通过自动 SWM 注册／取票、云端匹配及三项资产的 ticket 和文件校验。profile 11：loader 1,549,988 字节、digest 33,144 字节、sign 4,096 字节。未上传这组 loader，未写入分区。116 个常规测试通过，1 个忽略；Clippy 通过。
