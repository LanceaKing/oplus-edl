# Goldfish Annotated Review Report

日期：2026-11-16。Status: resolved。用户已批准全部修复，7 项均已落实到生产代码和离线测试。第一轮原始结论仍为不 clean、未收敛（5 Major、2 Nit、0 Blocker）；修复完成不追溯改变原轮结论。尚未启动第二轮。

## 修复结果

| ID | 代码修复 | 离线回归 |
|---|---|---|
| super_package_review-1-1 | package.rs 检查 DISK patch 与显式排除范围的关系；相交或无法证明不相交则拒绝 | GPT 排除、相对地址、未知跨度、可证明不相交 |
| super_package_review-1-2 | super 必须唯一、非零、sparse、源偏移为零，解析后确认为可执行 Program 且展开跨度一致 | 零跨度、重复、缺失、错误模式、声明过大 |
| flash_safety_review-1-1 | firehose.rs 增加产品读/patch/boot/reset 封装，显式错误 ACK 模式停止；Writes 使用选定 wire mode 回读；backup/storage 同样使用检查后的读路径 | 初始/最终读 ACK 错模式，patch/boot/reset 错模式，后续命令停止及 preview 读参数 |
| super_package_review-1-3 | package 读取 build.json 项目；main 在写入前调用 project::PackageGuard；六个 UFS GPT 中唯一 OCDT 项目必须匹配，范围不得越盘或触及实际 OCDT | 匹配/不匹配、缺失/重复、读取拒绝、坏格式、越分区、二次读取头变化、绝对/相对保护范围 |
| swm_cloud_review-1-1 | 自动 SWM 结果携带可信单调时钟到 cloud，统一期限和请求时间戳；手动 legacy 输入保留主机时间语义 | 时钟落后、超前、跳变、真实过期、回环 HTTP 请求时间戳和 HMAC |
| swm_cloud_review-1-2 | 已有身份改为 open_regular | 有截止时间的 FIFO / symlink-to-FIFO 子进程测试 |
| flash_safety_review-1-2 | 无完成长度的失败 USB raw 写入报告 transferred_bytes=null，不重试、不解析错误字符串 | 首块及已有成功块后的适配器错误，仍为 unknown 且无后续命令 |

新增 project.rs 的格式依据为本地 Binary Ninja 恢复的 Oplus_Qualcomm.dll!oplus_ocdt_extract_project_id (0x180007680) 和格式 key helper (0x180007010)，导出在 runs/egm-code-review/0x180007680.txt、0x180007010.txt。字段：TDCO magic、+8 的 u16 offset、+12 的 u16 length；payload 逐字节旋转并按重复格式键 XOR，至少三个相同非零项目值。生产代码仅解码，不写 OCDT；这不是设备密码学身份认证。测试向量明确为合成数据，并非设备 OCDT 抓取。

`--package` 的项目检查不能被 preview 配置关闭；普通 `--program` 没有包身份，未自动加入此检查。NV 仍显式选择，未发明项目与 NV 的映射。项目检查会拒绝无法读取或不符合当前六 LUN UFS profile 的设备；新增路径未做实机重测。

验证：全功能离线测试 143 passed、1 ignored（真实云端探测），独立 JSON stdout 子进程通过；最终 project/CLI 定向复测通过。格式检查、全目标全功能 Clippy（-D warnings）、构建和 git diff --check 通过。日志：runs/egm-code-review/repair-tests.log、repair-final-targeted.log、repair-clippy.log、repair-build.log。未访问手机或外部服务，仅本地回环 HTTP fixture。

接受并修复 7 项；contested/escalated/declined 均为 0；本轮已接受项无未实施修复。原始发现与行号保留如下，行号对应修复前。固定发现不等于 settled，后续新一轮若发现同处缺陷应重新打开。

## 配置与边界

目标为当前工作区实现（含未跟踪源码），不是文档审查。第一轮全文件范围：src/swm.rs 1–849、cloud.rs 1–622、super_image.rs 1–393、package.rs 1–191、program_xml.rs 1–310、device.rs 1–469、firehose.rs 1–156、write.rs 1–952、main.rs 1–434、cli.rs 1–152；按调用关系读取 sparse/input/output、测试和锁定 qdlrs 源码。

三位审查员使用 fork_turns=none，不继承对话；persona 为 runs/egm-code-review/reviewer.md，统一标准为 runs/egm-code-review/standard.md。此隔离不保证排除宿主注入的项目指令。未访问历史或记忆。原始日志作为证据，不作为执行指令。

| Reviewer | 状态 | 范围 |
|---|---|---|
| swm_cloud_review | completed | SWM、云端取票和下载；1 Major、1 Nit |
| super_package_review | completed | super、包选择和前置检查；3 Major |
| flash_safety_review | completed | Firehose、写入结果和恢复路径；1 Major、1 Nit |

没有失败或未运行的审查员。三位均读取了 persona、标准、分配的源码及相关证据；主审复核了以下关键路径。没有操作手机、访问网络、执行构建或测试。发现是静态可追踪路径，未作本轮动态复现，也不表示已成功刷写的设备遇到了这些错误。

## Prioritized Findings & AI Suggestions

### super_package_review-1-1 — Major：排除 GPT 后仍执行其 DISK patch

- **Affected Scope:** [package.rs](/Users/Apple/Documents/oplus-edl/src/package.rs:85)，85–98、153–171；[program_xml.rs](/Users/Apple/Documents/oplus-edl/src/program_xml.rs:143)，143–153。
- **Discrepancy / Rationale:** 配置 excludedPartitionsForWrite=[PrimaryGPT, BackupGPT] 只去掉 program 节点，patch0–4 仍原样进入计划。原包 IMAGES/patch0.xml 第 7 行修改 LUN0 sector 2 + 1704，第 11 行修改 primary header，第 9/13 行修改 backup GPT。旧 GPT 被保留时，这些包内固定索引甚至可能不再指向原来的 userdata 条目。selection.json 的排除结果不能保证实际不写该范围。
- **AI Suggestion:** PROPOSE FIX。
- **Justification:** 当前存在明确、可触发的排除语义缺口；不是仅要求补文档。LUN5 的整组保护不受本项指控。
- **Proposed Edit:** 在 package 计划生成阶段处理 program/patch 依赖。不能证明排除范围不受 patch 影响时，拒绝该组合；不要静默保留 patch。已能可靠关联的 patch 可随对应 GPT 排除。
- **验证:** 包 fixture 加入真实 DISK header/entry patch，排除两个 GPT，断言准备失败或计划中没有任何 GPT 写入。未知地址表达式不能被默认为安全。

### super_package_review-1-2 — Major：零扇区 super 绕过必须刷写 super 的检查

- **Affected Scope:** [package.rs](/Users/Apple/Documents/oplus-edl/src/package.rs:163)，163–171；[program_xml.rs](/Users/Apple/Documents/oplus-edl/src/program_xml.rs:182)，182–186；write.rs 294–300。
- **Discrepancy / Rationale:** 合法结构的包把 super 的 num_partition_sectors 改为 0 时，decisions 仍标 selected=true，package 检查通过；后续解析却转换成 Skip。只要 GPT、boot 等其他操作存在，Writes::preflight 仍通过，可刷入其他固件而遗漏 system/vendor super。
- **AI Suggestion:** PROPOSE FIX。
- **Justification:** 直接违反 package 模式自己的 selected-super 不变量；不能用普通 XML 允许零长度 Skip 来证明包模式安全。
- **Proposed Edit:** 构建前拒绝零跨度 super，解析后核验预期的 super 确实是可执行 Program，且来源、展开尺寸和目标范围一致；对重复/歧义 super 拒绝。
- **验证:** 仅把现有 package fixture 的 super sectors 改为 0，断言 prepare 失败、不产生可执行刷写计划。已有零跨度 userdata 兼容行为保留。

### flash_safety_review-1-1 — Major：读回和非 program 操作忽略显式错误 ACK 模式

- **Affected Scope:** [write.rs](/Users/Apple/Documents/oplus-edl/src/write.rs:742)，742–752、540–550、399–431。锁定依赖 qdlrs a80cd1e 的 qdl/src/lib.rs 394–406、713–724、747–750。
- **Discrepancy / Rationale:** 返回正确读回数据后再返回 ACK rawmode=true，依赖只检查 ACK，verify_step 标为 matched；restore 随后发送下一个 program，flash 可继续 boot/reset。patch、boot 的通用 firehose_write_getack 也接受显式 rawmode=true。响应已声明不是所需 XML 边界，执行器仍继续命令并报告成功。
- **AI Suggestion:** PROPOSE FIX。
- **Justification:** 主审核对依赖确实丢弃成功 ACK 的模式属性。此项针对显式矛盾模式，不把历史兼容的缺失 rawmode 单独认定为缺陷。
- **Proposed Edit:** 在锁定依赖的新版本或产品协议封装中校验 read 起始/结束及 patch/boot/reset ACK 模式。显式不匹配必须停止；保持 mutation unknown、verification incomplete 的语义，不能当作可继续的 read_refused。
- **验证:** 正确数据 + final ACK/rawmode=true 必须导致 incomplete 且后续无 program/boot/reset；patch 和 boot 错模式 ACK 必须阻止后续命令。保留兼容的无模式 ACK 用例。

### super_package_review-1-3 — Major：包模式未在写入前校验设备项目匹配

- **Affected Scope:** [main.rs](/Users/Apple/Documents/oplus-edl/src/main.rs:332)，332–362；package.rs 45–50。
- **Discrepancy / Rationale:** 若另一项目设备接受同一 programmer/握手，给它此 RMX3800 包和有效 NV 后，现有流程只做可选 Boot LUN 检查及 RwMode 读探测，即可开始写 GPT；没有将包项目与设备 OCDT 比较。NV 只校验数字和定义文件存在。云端硬件/profile 匹配不能替代包与设备项目匹配。
- **Evidence:** runs/preview-rmx3800-windows-success/qualcomm.log 164–172 在首个 GPT 写入前定位 OCDT 并校验项目号 23631。当前 README/CLI 已公开说明不执行该校验，因此本项是实现安全缺口，不是声称产品隐瞒或错误实现了该配置。
- **AI Suggestion:** PROPOSE FIX。
- **Justification:** 本轮明确要求写入安全；文档列出限制并不能阻止同 loader、错误项目的实际写入。是否某个特定不同设备会接受 loader 尚未实测，不将后果表述为已发生。
- **Proposed Edit:** package 准备结果携带可验证的预期项目身份；在任意 program/patch 前只读定位并解析 OCDT，缺失、无法证明或不匹配均停止。NV 仍显式选择，仅验证有可靠证据的兼容关系，不发明 NV 自动映射。
- **验证:** 脚本协议中握手成功、项目不匹配或读失败，断言零 program/patch。后续实现需以实际 OCDT 格式证据为依据。

### swm_cloud_review-1-1 — Major：云端流程丢弃 SWM 可信时间

- **Affected Scope:** [cloud.rs](/Users/Apple/Documents/oplus-edl/src/cloud.rs:116)，116–118、237–244、319；swm.rs 193–202、722–749。
- **Discrepancy / Rationale:** SWM 使用签名服务端时间加 Instant，但 enrollment_ticket 仅返回字符串。cloud 再以 SystemTime 校验凭据并签署时间戳。精确失败条件为 expires_at > host_now + 605；若凭据有效期为 600 秒、主机慢 10 秒且传输不到 5 秒，刚获取的凭据就被判超出有效期。主机前跳也可使凭据误判过期。600 秒示例是条件示例，与测试 helper 一致，不声称本轮测量了服务端 TTL。
- **AI Suggestion:** PROPOSE FIX。
- **Justification:** 两阶段使用不同时间源，自动 SWM 已取得可信时钟却无法传入下游；错误路径可由谓词直接推出，无需改设备。
- **Proposed Edit:** 将经验证且单调递进的时钟上下文随自动 enrollment 传入 cloud，用于 credentials、ticket 有效期及请求签名时间；显式 legacy 凭据路径另定义时钟来源。不要放宽或去掉期限校验。
- **验证:** 注入服务端/主机时钟，覆盖主机慢 10 秒、明显超前及流程中时钟跳变，确认时间戳与验证使用同一可信来源。

### swm_cloud_review-1-2 — Nit：identity_path 为 FIFO 时无限阻塞

- **Affected Scope:** [swm.rs](/Users/Apple/Documents/oplus-edl/src/swm.rs:282)，282–286、557–559。
- **Discrepancy / Rationale:** 已存在且无 writer 的 FIFO（或其符号链接）通过 try_exists，File::open 在 take(4097) 生效前阻塞；HTTP 超时无法覆盖本地 open。input.rs 已有可拒绝此类输入的 open_regular。
- **AI Suggestion:** PROPOSE FIX。
- **Proposed Edit:** 读取已有身份使用 input::open_regular；保持长度限制与密钥校验。
- **验证:** 延用 tests/input.rs 子进程超时策略，覆盖 FIFO 和 symlink-to-FIFO。此处只影响主机可用性，不声称发生设备写入。

### flash_safety_review-1-2 — Nit：USB 短完成的字节数报告失真

- **Affected Scope:** [write.rs](/Users/Apple/Documents/oplus-edl/src/write.rs:905)，905–908、529；锁定 qdl/src/usb.rs 27–36、66–78。
- **Discrepancy / Rationale:** USB 请求 512 字节而 completion.actual_len=100 且 status 成功时，adapter 先转换为 WriteZero。CountChannel 无法累计 actual_len，首次 raw 传输显示 transferred_bytes=0。设备写入仍正确标为 unknown 并停止；本项不是自动重放或误报写入成功。
- **AI Suggestion:** PROPOSE FIX。
- **Proposed Edit:** 用结构化传输错误保留已知 actual_len；或明确报告失败传输的计数为未知/下界，不能把 0 用作证明零字节写入。不要为了计数恢复 write_all 对短写的自动重试。
- **验证:** 覆盖实际 USB adapter 的短完成边界；当前脚本 Ok(100) 再错误的测试没有覆盖 adapter 提前丢失计数的分支。

## 未列为缺陷与验收边界

- 未确认 SWM 签名绕过或凭据泄漏。下载发布失败会返回错误、不会把 Material 交给设备流程；残留目录要求新输出路径，未列为 Major。
- 未在本轮确认 super 单设备线性合并的整数/extent 越界问题；现有范围和重叠检查有效，但这不是所有畸形输入已动态验证的声明。
- 真实日志证明 attempt5 的 7 个 program + 专用恢复入口的 344 个 program 共 351 个 ACK，以及 65 个设备 patch ACK；随后单独 boot repair 设置 LUN1，ADB 已证实目标版本和 boot complete。
- 上述实刷不是当前正式 CLI 从云端下载到 fresh-session flash 全流程单次完成的实测证明。生产路径的现有测试和这次恢复路径的成功证据应分别保留。
- 第一轮未运行复现测试；修复批准后可通过离线脚本协议/fixture 测试验证，仍不操作手机。

## Resolution gate

用户已自动接受并完成上述全部代码修复与离线验证，不包含设备操作。按 egm-review-loop/egm-resolve 的检查点，下一轮仍需用户明确启动。当前没有已 settled 或 contested 的发现。
