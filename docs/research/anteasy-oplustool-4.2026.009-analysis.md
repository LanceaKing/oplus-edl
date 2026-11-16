# Anteasy OplusTool 4.2026.009 预览版分析

> 后续进展：本文的静态分析与早期匿名访问结果之后，preview 云端配套资产路径已有 RMX3800 实刷和启动证据；不据此推定所有机型或原包授权路径可用。详见[实刷记录](../implementation/rmx3800-cloud-flash-20261116.md)；总览见[研究索引](README.md)。

分析日期：2026-11-15 UTC。本文针对 Anteasy 发布的第三方 **OplusTool**，不是 realme 官方的 **OplusFlashTool 5.9.18.0**。两者的名称、版本和服务端授权不能混用。

后续核查见 [RMX3800 原包会话授权](rmx3800-session-authorization.md)：已将实机挑战与 14.1.0.190 programmer 对齐，确认它没有本报告 16.0 对照样本的显式 getsigndata 准备标志检查，但仍有 RSA 验签。另发现公开客户端的第三方签发入口；经单独授权的一次请求返回“账户不存在”，未取得签名。Anteasy 正常客户端的云端资产路径仍未排除，匿名列表 401 不等于原版客户端不可用。

## 1. 问题与结论

本次问题是：这个预览版如何完成设备授权，它能否解决 RMX3800 / SM8650 在官方 OplusFlashTool 中遇到的认证依赖？

已恢复的一加离线路径通过本地 `demacia` 和 `setprojmodel` token 完成旧 programmer 的设备认证。它与此前 MSMDownloadTool 5.1.75.0 / 5.1.77.0 的两类 token 属于同一协议族。该路径不能直接替代官方 5.9.18.0 的 `getsigndata → RCSM /api/tools/sign → verify`。

此外，客户端包含 Anteasy 自己的 SWM 操作授权和云端 loader 服务。“设备 token 在本机计算”与“软件完全不需要服务器”是不同命题。不能把“一加离线授权”理解为所有 OPPO / realme 型号均可离线刷写。

**本次没有证明 RMX3800 可免 RCSM 认证刷写。** 继续追踪确认云端匿名列表返回 401；官方 RMX3800 包内 loader 的设备签名与 VIP 门槛仍在，3 组 OSIG 输出与官方 5.9.18.0 原生主机端逐字节一致。第 10 节定位离线 SHA 失败原因，验证 SHA、RSA 和固定公钥下的 Y/N 拒绝路径；第 11 节恢复 getsigndata 的五类字段来源；第 12 节追踪签名表的 SMC 边界，离线验证官方表的 ECDSA 签名，并定位 DigestRead 对应的 dinfo 持久记录。安全世界实际接受、厂商 RSA 设备授权及 DigestRead 成功路径仍未动态验证。发布说明只声明离线授权支持一加 7 / 8 / 9 系列的部分设备。客户端确实有 `Realme / GT6 / RMX3800` 目录项，附带数值 `23631`，但目录项不等于可用的 loader 或设备验签通过。

## 2. 获取与样本身份

来源：[Anteasy 下载页](https://oplustool.anteasy.com/downloads)。匿名公共 API `GET /api/public/releases?type=preview&page=1&page_size=10` 返回的最新可用预览版为 `4.2026.009`，release ID `48`，发布日期 `2026-09-29`。过滤结果内更早的版本是 `4.2026.008`。记录中的 `is_latest=false` 是接口的全局标记，本文没有将它改写为 true。

通过该发布记录的公开下载接口取得 ZIP，没有登录或调用业务授权服务。包内 939 个条目的 CRC 校验均通过。CRC 和自行计算的 SHA-256 用于固定本次样本身份，不等于厂商数字签名认证。

| 文件 | 字节数 | SHA-256 |
|---|---:|---|
| `OplusTool_preview_v4.2026.009.zip` | 152587342 | `3ef05ca508d57b1f9c23668c87f4400460c3af420a4ffc38b3426b1b5d1a0497` |
| `OplusTool.exe` | 60755968 | `cc7af93dee955fcb659a5dfb146bd9c30412502f5269b5f90331982130760577` |
| `Oplusdll/Oplus_Qualcomm.dll` | 14511104 | `05562025f23535bf291a5c19ba5a4bfb792a5945a2f67526dc8eb118b8ca1ca5` |
| `Oplusdll/MySwm.dll` | 15310336 | `3b4c6dd0b2b5b5b774cd49562462191a44d859b264ff79e3c414a724343dc1a5` |
| `Oplusdll/loader.dll` | 14266368 | `f1e71749bb457c69919c29480acaaf19625d499f45cb498904b0049ca6251aa3` |

EXE 的 FileVersion 为 `4.2026.009`，Assembly Version 为 `4.2026.9.0`。样本均为 x86-64 PE；EXE 没有 CLR directory，但保留 .NET 类型、异步状态机及 Avalonia 元数据，符合 NativeAOT 程序的特征。Qualcomm、SWM、loader DLL 有可读原生代码，也有高熵附加段和受保护的调用入口；本次没有声称这些模块已完整脱壳或去虚拟化。

证据：[下载记录](anteasy-oplustool-4.2026.009-evidence/acquisition.json)、[版本列表](anteasy-oplustool-4.2026.009-evidence/releases-preview.json)、[发布详情](anteasy-oplustool-4.2026.009-evidence/release-48.json)、[ZIP 清单](anteasy-oplustool-4.2026.009-evidence/zip-members.json)、[PE 清单](anteasy-oplustool-4.2026.009-evidence/pe-inventory.json)。

原始 ZIP 保存在 [runs 样本目录](../../runs/anteasy-oplustool-preview/sample/OplusTool_preview_v4.2026.009.zip)。同一发布页的驱动安装器不在本次下载、执行或分析范围内。

## 3. 方法与证据边界

使用实际 Binary Ninja GUI 中的 Python API 加载样本，版本 `5.2.8614 Personal`。通过 [分析桥接脚本](../../runs/anteasy-oplustool-preview/bn_bridge.py) 导出函数、交叉引用、HLIL 和汇编；没有用字符串扫描替代 Binary Ninja 分析。GUI 原有数据库保持不变。

四个视图的初始分析已完成：EXE 114466 个函数，Qualcomm DLL 7493 个，SWM DLL 20053 个，loader DLL 16273 个。数量包含运行库、受保护区域与自动识别结果，不代表已逐一恢复全部函数语义。主程序全量分析期间，使用独立导出通道读取已完成的模块；等待超时的请求在完成后恢复导出，没有把等待超时当成分析失败或认证失败。

以下地址均为对应模块的 VA，DLL ImageBase 为 `0x180000000`，EXE 为 `0x140000000`。跨 DLL 相同 VA 不代表相同函数。自动类型和签名识别可能不准确，关键参数以汇编和局部执行读回为准。

首轮局部执行只运行 token 序列化函数；本轮追加的官方 loader OSIG 差分及其执行边界见第 9 节。内存操作、SHA-256 和 AES 包装函数使用明确记录的标准实现适配器；没有执行受保护的 Windows CNG 调用、进程初始化、SWM 授权、USB 访问或设备写入。这些结果证明指定输入下的序列化和输出格式，不证明服务端或真实 programmer 接受请求。

## 4. 一加设备离线认证

### 4.1 输入与状态机

`Oplus_Qualcomm!oplus_qcom_oneplus_auth_context_create @ 0x18004be60` 检查输入结构大小 `0x40`，复制项目字节串和可选 pk；pk 必须为空或恰好 16 字节。上下文还保存两个 uint32 和一个 uint64：沿 token 调用链可确定分别对应 SN、CF 和时间值。时间为 0 时取本地时间；没有传入 pk 时以时间种子调用 `srand/rand`，从 62 字符表生成 16 个字符。

| 上下文偏移 | 含义 |
|---|---|
| `+0x00` | 项目字符串 P |
| `+0x20` | pk 字符串 |
| `+0x40 / +0x44` | uint32 SN / CF |
| `+0x48` | 时间值 TS |
| `+0x50 / +0x54` | action / 处理状态 |
| `+0x58 / +0x78` | 两个 XML 命令的字符串缓存 |
| `+0x98` | 响应缓存 |
| `+0xb8 / +0xbc / +0xc0` | verify_res / model_check / auth_token_verify |

主链为：创建上下文，生成并发送 demacia，累积其响应，再生成并发送 setprojmodel，累积其响应。`copy_command @ 0x18004c680` 生成命令并转换等待状态；`feed_response @ 0x18004c7a0` 限制累计响应不超过 1 MiB，等待 `</data>`，将 NAK 变成失败状态。

在两个响应处理器中，`value="ack"` 是可见的主要通过条件。`0x1800038a0` 提取 verify_res；`0x180003a90` 提取 model_check 和 auth_token_verify。这里没有把这些诊断字段全部强制比较为 0。EXE 的主循环 `0x14059e240` 最多迭代 8 次：action 1/3 生成并发送命令，feed_response 状态 0/`0x22` 允许继续，action 5 返回认证步骤成功，其他失败进入 `0x14059e820` 的错误诊断。该成功仅属于这一步，不等于后续刷写完成。

证据：[上下文创建](anteasy-oplustool-4.2026.009-evidence/qcom/hlil/0x18004be60.txt)、[命令生成](anteasy-oplustool-4.2026.009-evidence/qcom/hlil/0x18004c680.txt)、[响应累积](anteasy-oplustool-4.2026.009-evidence/qcom/hlil/0x18004c7a0.txt)、[demacia 响应](anteasy-oplustool-4.2026.009-evidence/qcom/hlil/0x1800038a0.txt)、[项目响应](anteasy-oplustool-4.2026.009-evidence/qcom/hlil/0x180003a90.txt)。

### 4.2 token 的序列化与密码接口

`demacia` 的序列化位于 `0x180002a10`，`setprojmodel` 位于 `0x180002cd0`。两者最终形成 256 字节的零填充明文，将 32 字节密钥和 16 字节 IV 传给 `0x180001d00`，再把 256 字节输出编码成 512 个大写十六进制字符。命令构造器分别为 `0x180003410` 与 `0x180003640`。

demacia 使用十位十进制 SN、固定前后缀、SHA-256 原始摘要和 `907heavyworkload` 标记。其密钥前后缀、IV 与旧 MSM 报告中的 demacia 相同。短 SN 的补零不是可选格式。

setprojmodel 使用旧 MSM 的逗号字段顺序与密钥/IV，但自行提供固定 profile：

```text
M = "TdYWNqblwrWOJ1Yy"
W = "Oplus_downloadtool"
CUST = UHex(SHA256("7016147d58e8c038" || P || M))
H = UHex(SHA256("c4b95538c57df231" || P || u32(CF) || u32(SN)
               || W || TS || CUST || "5b0217457e49381b"))
message = join(",", P, M, CUST, W, u32(CF), u32(SN), TS, H)
plaintext = message 后补零至 256 字节
```

这是**指定固定 profile 下的协议重建**，不是逐字复制任意官方固件的全部配置。不能由此推导所有旧 programmer 或所有项目都接受它。完整旧协议的密钥、IV 和三类 token 区别见 [认证协议 §3](msmdownloadtool/auth.md)。本样本已恢复的离线链只使用 demacia / setprojmodel，没有证明它实现了同样的 setswprojmodel 或新版 RCSM 签名。

密码包装层可见 `SHA256`、`ChainingModeCBC`、32 字节密钥、16 字节 IV，以及无填充的加密调用参数；实际 Windows API 经受保护跳板调用。证据：[demacia 原生序列化](anteasy-oplustool-4.2026.009-evidence/qcom/asm/0x180002a10.txt)、[setprojmodel 原生序列化](anteasy-oplustool-4.2026.009-evidence/qcom/asm/0x180002cd0.txt)、[SHA 包装](anteasy-oplustool-4.2026.009-evidence/qcom/hlil/0x1800016d0.txt)、[CBC 包装](anteasy-oplustool-4.2026.009-evidence/qcom/asm/0x180001d00.txt)。

### 4.3 局部执行验证

[验证脚本](../../runs/anteasy-oplustool-preview/validate_token_serialization.py) 在 Unicorn x86-64 中执行原始序列化和十六进制编码机器码，包含 4 组输入、2 类 token，共 **8 个案例**。覆盖 SN 为 0、12、普通值、`0xffffffff`，CF 为 0、1、2、`0xffffffff`，以及多个项目和时间值。

8/8 案例的 SHA 输入、AES 明文、密钥、IV、最终编码输出均与独立公式一致；原生函数均正常返回成功，输出长度均为 512。SHA 包装由 hashlib 替代，AES 包装由 OpenSSL AES-256-CBC 无填充替代；因此这不是“原始 CNG 密码实现已执行成功”，也不是本样本与旧 MSM 的完整原生执行差分。

例如 P=18825、SN=12、CF=0、TS=1700000000 时，demacia 的摘要输入含 `0000000012`；项目明文的字段顺序为 `P,M,CUST,W,CF,SN,TS,H`。逐例原始中间值和适配器命中次数见 [验证结果](anteasy-oplustool-4.2026.009-evidence/token-serialization-validation.json)。

## 5. SWM 软件授权与云端 loader

本节区分软件运营方的授权与手机 programmer 的设备认证。SWM 的 operation grant 不是 RCSM 的 token，也不是 demacia 的 pk。

### 5.1 EXE 中的前置授权

`OplusTool!0x1405a0d40` 是已恢复的异步授权入口。有效项目号且存在服务对象时，它记录 `requesting SWM authorization before offline signature generation`，调用 `0x1405a6ea0`，等待结果后才进入 `0x14059e240` 的设备认证循环。没有服务对象则记录 `SWM operation authorization service is unavailable; the offline signature was not generated` 并返回错误。

`0x1405a6ea0` 构造以 `oneplus.offline_auth:` 开头的操作描述，把操作枚举 **5** 传给授权适配层。`MySwm!0x18000a680` 将枚举 5 映射为 `qualcomm.oneplus_offline_auth`。因此 SWM 是这条正常客户端路径的前置步骤，不是仅在无关更新代码中出现的字符串。

该入口在项目为空时有直接调用设备 helper 的分支，但 `0x14059e240` 随即检查空项目并报错。不能把这条分支当成免授权成功路径。

证据：[EXE 授权状态机](anteasy-oplustool-4.2026.009-evidence/app/hlil/0x1405a0d40.txt)、[操作描述与枚举](anteasy-oplustool-4.2026.009-evidence/app/hlil/0x1405a6ea0.txt)、[设备认证循环](anteasy-oplustool-4.2026.009-evidence/app/hlil/0x14059e240.txt)、[SWM 操作映射](anteasy-oplustool-4.2026.009-evidence/swm/hlil/0x18000a680.txt)、[托管字符串读回](anteasy-oplustool-4.2026.009-evidence/app-literal-readback.json)。

### 5.2 SWM 请求与消费

`MySwm.dll` 包含 `qualcomm.flash`、`qualcomm.erase`、`qualcomm.modify`、`qualcomm.read`、`qualcomm.oneplus_offline_auth` 等独立操作名；公共导出包含 `oplus_swm_client_begin_operation_authorization_v3`。可见请求 schema 为 `operation_grant_v3_host` / `operation_grant_v3_unbound`，包含 operation、plan_sha256、step_count、total_bytes、consumer_challenge，以及主机/模块完整性绑定字段。

导出 `0x18005e090` 调用构造器 `0x180004d90`。任务分派 `0x180046e80` 的请求类型 7 选择 `POST /api/client/operation-authorizations`，包含离线时长超限、challenge 不匹配、主机完整性绑定不匹配及设备/密钥绑定不匹配等失败出口。随后 `0x180041290` 使用 `/api/client/operation-authorizations/consume`。此处的“请求类型 7”与上文的“操作枚举 5”是两套枚举。

这些检查表明软件还有自己的授权状态与操作消费协议。本次没有完整恢复全部 grant 验签核心，也没有推断服务器会签发何种权限。

证据：[授权导出](anteasy-oplustool-4.2026.009-evidence/swm/hlil/0x18005e090.txt)、[JSON 构造](anteasy-oplustool-4.2026.009-evidence/swm/hlil/0x180004d90.txt)、[请求分派与响应门槛](anteasy-oplustool-4.2026.009-evidence/swm/hlil/0x180046e80.txt)、[消费请求](anteasy-oplustool-4.2026.009-evidence/swm/hlil/0x180041290.txt)。

### 5.3 云端 loader 的独立路径

云端 loader 模块的默认 origin 是 `https://oplustool.anteasy.com`。其可见接口为 `/api/v1/cloud-loader/enroll`、`/list`、`/ticket`。列表请求使用 platform、msm_id、pk_hash_sha384、pk_hash_sha256、scheme、device_code 等筛选字段；票据结构还绑定 profile_id、asset_id、文件类型、大小、SHA-256、下载 URL 摘要和到期时间。

Binary Ninja 已确认实际构造与发送位置：enroll 在 `0x18003f3b0`，list 在 `0x18003ba10`，ticket 在 `0x18003abe0`。`0x180044910` 解析并检查 `cloud_loader_ticket_v2` 及所需字段，`0x180049d00` 检查下载数据大小和 SHA-256，存在票据大小超限、提前结束、摘要不符和载荷格式不符的失败出口。没有证据表明把界面型号改成 RMX3800 就会产生可被设备接受的 loader。

证据：[列表请求](anteasy-oplustool-4.2026.009-evidence/loader/hlil/0x18003ba10.txt)、[票据请求](anteasy-oplustool-4.2026.009-evidence/loader/hlil/0x18003abe0.txt)、[enroll](anteasy-oplustool-4.2026.009-evidence/loader/hlil/0x18003f3b0.txt)、[票据检查](anteasy-oplustool-4.2026.009-evidence/loader/hlil/0x180044910.txt)、[资产校验](anteasy-oplustool-4.2026.009-evidence/loader/hlil/0x180049d00.txt)。

首轮服务端接口名称来自本地样本，未调用业务 API。本轮只对列表接口作了一次匿名只读请求，返回 401；详细记录见第 9 节。**两轮均未领取授权、消耗额度或下载云端 loader**。公共 ZIP 下载成功不能证明拥有上述服务权限，也不能证明云端有适配 RMX3800 当前 PK hash 的 loader。

辅助定位材料：[关键词记录](anteasy-oplustool-4.2026.009-evidence/keyword-inventory.json)、[SWM 原始字符串](anteasy-oplustool-4.2026.009-evidence/MySwm-raw-strings.json)、[loader 原始字符串](anteasy-oplustool-4.2026.009-evidence/loader-raw-strings.json)。未搜到某个明文字符串不能证明受保护代码中不存在相应功能。

## 6. 与官方 5.9.18.0 的关系及 RMX3800 方案

`OplusTool!0x1407681d0` 建立型号目录，RMX3800 项的三个字符串依次是 `Realme`、`GT6`、`RMX3800`，在对象 `+0x20` 写入 `0x5c4f = 23631`。这与此前官方包中 `Projectconfig.xml` 的程序项目值一致；该目录字段的全部消费者尚未审计，不能把它等同于设备实际返回的项目或公钥哈希。

证据：[型号目录](anteasy-oplustool-4.2026.009-evidence/app/hlil/0x1407681d0.txt)、[目录字符串读回](anteasy-oplustool-4.2026.009-evidence/app-literal-readback.json)。

| 问题 | 本次可得结论 |
|---|---|
| 是否实现旧一加本地 token？ | 是，已恢复两类序列化及响应状态机，并做了限定范围的 8 个执行案例。 |
| 是否摆脱所有网络授权？ | 不能这样表述；程序还有 SWM 操作授权和云端 loader 服务。 |
| 能否给 RMX3800 直接使用这套 token？ | 尚无证据；官方 RMX3800 / SM8650 分析链使用另一套设备签名协议。 |
| 云端 loader 能否解决？ | 这是需要进一步核验的候选路径，取决于具体 loader、设备身份匹配和 programmer 的实际认证行为。 |
| EXE 有 RMX3800 目录项是否足够？ | 不足够；已确认目录及 23631 数值，仍缺具体 loader、PK hash 和设备握手证据。 |

对已经确认走 RCSM / SFP 的官方 RMX3800 流程，现有证据支持的路径仍是使用有效售后授权完成签名。若另一个 programmer 能被该设备的安全启动链接受，且其 Firehose 不要求该 RCSM 流程，则它可能形成不同路径；**同为 SM8650、loader 下载成功或工具显示型号，都不足以证明这件事**。

要把候选路径变成可验证结论，需要先获得该设备的 Sahara 身份和 PK hash，核对具体 loader 的哈希、签名及匹配条件，再分别验证 PBL 接受、Firehose 建链、设备授权、分区读写与启动。本文不把前一阶段成功当成后一阶段成功。

对照：[官方 OplusFlashTool 5.9.18.0 报告](oplusflashtool-5.9.18-analysis.md)、[认证协议](msmdownloadtool/auth.md)、[刷写协议](msmdownloadtool/flash.md)。

## 7. 尚未确认的部分

1. RMX3800 当前设备身份、PK hash、DDR 与具体 programmer 的兼容关系。
2. 云端是否实际提供匹配资产，以及有效票据、权限、额度和当前服务端规则。
3. 受保护代码与 Windows CNG 实际运行结果；局部标准密码适配器不能代替这部分证据。
4. NativeAOT 主程序中其他刷写、恢复、MTK、Fastboot 分支的完整恢复；已审计的认证循环不代表覆盖全程序。
5. 真实 programmer 对固定 M/W/CUST profile 的接受范围；部分一加 7/8/9 的支持声明不等于逐机型实测。
6. 真实设备的刷写、读回和启动验证。本次没有执行这些操作。

## 8. 保存的分析产物

- Binary Ninja 数据库：[EXE](../../runs/anteasy-oplustool-preview/bn/app.bndb)、[Qualcomm](../../runs/anteasy-oplustool-preview/bn/qcom.bndb)、[SWM](../../runs/anteasy-oplustool-preview/bn/swm.bndb)、[loader](../../runs/anteasy-oplustool-preview/bn/loader.bndb)。
- [产物验证记录](anteasy-oplustool-4.2026.009-evidence/artifact-validation.json)固定样本哈希、局部验证计数和文档链接检查。
- [证据文件清单](anteasy-oplustool-4.2026.009-evidence/evidence-manifest.json)固定本次导出的证据内容。


## 9. 云端 loader 与 RMX3800 对照样本的继续追踪

本轮日期：2026-11-15 UTC。结论分为两部分：**Anteasy 的 RMX3800 云端资产尚未取得，不能判断它是否免 RCSM；已取得的官方 RMX3800 loader 保留 OSIG 签名验证及读写权限状态，其序列化与官方 5.9.18.0 主机端相符。** 后者是官方包对照，不能冒充前者。

### 9.1 SWM 票据、临时凭据、资产票据是三种对象

| 阶段 | 本轮确认的代码与数据 | 边界 |
|---|---|---|
| SWM enrollment ticket | `MySwm!oplus_swm_client_begin_enrollment_ticket @ 0x18005dd30` 将任务类型设为 6；`0x1800451d0 / 0x180046e80` 选择 `POST /api/client/enrollment-ticket` | 不是第 5 节的操作类型 5，也不是任务类型 7 的 operation grant |
| EXE 建立 loader 上下文 | `OplusTool!0x14077b7a0` 检查票据非空及正整数 bootstrap 值；`0x14077f860` 保存两者；`0x140784600 → 0x1407814d0` 构建 native 上下文 | bootstrap 绑定的受保护实现仍未完全恢复，不把这个整数解释成可自造的授权值 |
| enroll 换取临时凭据 | `loader!0x18003f3b0` 发送 `ticket, app_id, release_id, app_version`；`0x18003ebb0` 解析 `client_id, client_secret, delegation_id, expires_at` | 这些字段来自 enroll 响应；release/app UUID 是公开身份字段，不是替代 client_secret 的凭据 |
| 凭据时效与请求签名 | `0x180036030` 要求三个字符串非空且未到期；enroll 还要求到期时间不晚于当前时间加 `0x25d = 605` 秒。`0x180056bc0` 构建 `X-Client-ID / X-Timestamp / X-Nonce / X-Signature-Version / X-Signature` | 客户端接受窗口不等于服务器一定签发 605 秒；签名包装进入受保护的 Windows CNG 调用，未作完整原生执行 |
| 列表、下载票据、资产 | `0x180040ed0` 先取得凭据，再签名 POST `/list` 或 `/ticket`；后续校验 `cloud_loader_ticket_v2`、绑定字段、到期时间、大小、SHA-256 及载荷格式 | 下载授权不产生手机端 OSIG 签名，也不证明 PBL 接受该资产 |

SWM 分派器实际包含 `X-SWM-DPoP`、`X-SWM-Body-Enc: x25519-aes-gcm-v1`、`X-SWM-Online-Key-Id`，并在有会话时附加 `X-SWM-Session`，目标 origin 为 `https://swm-backend.anteasy.com`。这里只确认字段和调用分支，没有通过服务端认证。`loader!0x18003ee40` 缓存已 enroll 的临时凭据；过期会清理凭据并返回 `AUTH_EXPIRED`，要求重建上下文，不会把旧凭据视为永久可用。

证据：[SWM 导出汇编](anteasy-oplustool-4.2026.009-evidence/cloud-loader/swm/asm/0x18005dd30.txt)、[任务与路径映射](anteasy-oplustool-4.2026.009-evidence/cloud-loader/swm/hlil/0x1800451d0.txt)、[SWM 请求分派](anteasy-oplustool-4.2026.009-evidence/swm/hlil/0x180046e80.txt)、[EXE bootstrap 状态机](anteasy-oplustool-4.2026.009-evidence/cloud-loader/app/hlil/0x140784600.txt)、[enroll 响应字段](anteasy-oplustool-4.2026.009-evidence/cloud-loader/loader/hlil/0x18003ebb0.txt)、[凭据缓存](anteasy-oplustool-4.2026.009-evidence/cloud-loader/loader/hlil/0x18003ee40.txt)、[请求签名](anteasy-oplustool-4.2026.009-evidence/cloud-loader/loader/asm/0x180056bc0.txt)。

### 9.2 资产匹配不是单凭 RMX3800 或 23631

EXE 的自动匹配状态机 `0x1405ba2a0` 从 Sahara 身份对象构建 key，调用 `0x14077f8f0`。该 helper 从提供的 PK hash 字节区取前 32 / 48 字节生成字符串，并格式化 MSM ID；这是对设备已提供的 hash 编码，不能解释为对 loader 文件重新计算 SHA-256/384。输入不足相应长度时对应字段为空。状态机以 `qualcomm` 平台调用列表，随后处理零项、单项、多项结果；零项报无匹配，多项不直接采用第一项。

单项结果的 scheme 与 `DIGEST` 作不区分大小写比较。代码先下载 `loader`，DIGEST 分支再取 `digest` 和 `sign`；三种类型分别来自同一被选 profile 的资产字段。已读回 NativeAOT 字符串对象，避免把字符串存储地址与对象地址混淆。静态 catalog 的 `RMX3800 / 23631` 尚未被证明就是服务端要求的 `device_code`，也没有服务端回包把该型号绑定到某个 profile/asset ID。

实测只进行一次匿名只读列表请求：`POST https://oplustool.anteasy.com/api/v1/cloud-loader/list`，body 使用 `platform=qualcomm, device_code=23631, scheme=NORMAL`，未知的 PK hash 与 MSM ID 留空；没有 cookie 或签名凭据。服务于 **2026-11-15 05:37:56 UTC** 返回：

```json
{"code":"signature_required","error":"缺少签名请求头","success":false}
```

HTTP 状态为 401。它证明本次匿名请求在签名门槛被拒，**不证明 23631 是正确筛选值、不证明匹配列表为空，也不证明 DIGEST 或 NORMAL 中哪一种可用**。未调用 enroll、领取/消费票据或请求资产；用户确认没有现成授权会话或缓存，后续保持公开样本与离线协议分析。

公开检索还检查了 [bkerler/edl #692](https://github.com/bkerler/edl/issues/692)：其中明确注明的 SM8650 附件来源是 **RMX3888 GT5 Pro**，后续 RMX3800 内容是索取该型号文件的评论。这不足以作为 RMX3800 的已匹配云端资产。本轮没有把跨机型附件加入已验证样本。

证据：[完整 HTTP 记录](anteasy-oplustool-4.2026.009-evidence/cloud-loader/anonymous-list-attempt.json)、[自动匹配与分件下载](anteasy-oplustool-4.2026.009-evidence/cloud-loader/app/hlil/0x1405ba2a0.txt)、[PK hash key 构造](anteasy-oplustool-4.2026.009-evidence/cloud-loader/app/hlil/0x14077f8f0.txt)、[字符串对象读回](anteasy-oplustool-4.2026.009-evidence/cloud-loader/app-cloud-literals.json)。

### 9.3 官方 RMX3800 loader 的来源与 Binary Ninja 分析

对照样本来自 realme 官方服务包 [RMX3800domestic_11_16.0.5.740CN01_2026041411410326.zip](https://rms11.realme.net/sw/RMX3800domestic_11_16.0.5.740CN01_2026041411410326.zip) 内的 `IMAGES/prog_firehose_ddr.elf`。它此前通过 HTTP Range 提取，本轮复核本地字节和哈希，未重新完整下载服务包。

| 对象 | 字节数 | SHA-256 |
|---|---:|---|
| 原始 programmer | 1568936 | `daaddec2d87fa2f30a527f0e3d656044ef9e2fdece8163afde856cb9505e34ea` |
| 从文件偏移 `0x110b50` 到 EOF 的分析副本 | 451928 | `f2eca101d4a1c4247fc196052dc5eba57ffcaecf2dab99dc64b6282c48940622` |

分析副本用于加载其中的 AArch64 ELF，不是重新封装的可刷写 programmer。Binary Ninja 5.2.8614 Personal GUI 实际加载该副本，识别 1852 个函数；本轮导出的 HLIL、汇编及 refs 与此前 Capstone 证据相互核对。以下地址均为嵌套 ELF 声明 VA。若 HLIL 将共享尾部或栈检查失败后的相邻函数合并，按实际汇编控制流判断，不把反编译尾调用当作真实正常返回路径。

命令表 `0x149cd000` 已解析出 30 项，其中 `getsigndata → 0x149a19b8`、`verify → 0x149a1a6c`、`program → 0x149a27fc`、`read → 0x149a29e8`、`sha256init → 0x149a2554`。不是仅根据字符串判断命令存在。

证据：[来源与 ELF 程序头](anteasy-oplustool-4.2026.009-evidence/cloud-loader/official-loader-identity.json)、[BN 元数据](anteasy-oplustool-4.2026.009-evidence/cloud-loader/rmx3800/metadata.json)、[命令表与常量](anteasy-oplustool-4.2026.009-evidence/cloud-loader/rmx3800-command-table.json)、[BN 数据库](../../runs/anteasy-cloud-loader/rmx3800-firehose-a64.bndb)、[前期官方包分析](../implementation/phase-6-resources/offline-analysis.md)。

### 9.4 从设备挑战到读写权限

| 环节 | 新证据 | 判断 |
|---|---|---|
| 获取挑战 | `0x149a19b8` 取得芯片序列号、调用 `0x149a0d3c` 准备 Rand，读取旧记录准备 DigestRead/NV，调用 `0x149a1150` 输出字段，最后设置 `0x8025b280` 的准备状态。第 11 节确认 DigestWrite 来自前序表缓存 | 输出字段为 Version、Platform1/2、Mode、Rand、ProjectWrite/Read、DigestWrite/Read、ID、NV、SecureBoot；与官方客户端解析字段吻合 |
| 固定平台字段 | Version=`0`、Platform1=`QCOM`、Platform2=`SM8650`；Mode 取 W/R；ProjectWrite/Read 在此样本输出 `00000` | 不能为了“对应 23631”而把 OSIG 内两个项目字段改成 23631；不同层的项目值不可混用 |
| verify 序列 | `0x149a1a6c` 要求准备状态为 1，再进入 `0x149a1414` | 任意发送 verify 不能建立正确前序状态 |
| 传输长度 | `0x149a1414` 接收 `0x1000` 字节，再把前 `0x100` 字节作为签名输入传给 `0x149a1224` | 4096 字节传输帧与 256 字节 RSA 签名是不同边界，与官方 SFP 客户端观察相符 |
| 签名检查 | `0x149a1224` 生成 `0xd0` 字节 OSIG，先使用 Y，失败再尝试 N；`0x149a4610` 经摘要包装进入 RSA 检查 | 成功分支设置 AUTH_INTRANET=1 或 AUTH_INTERNET=2；两次失败返回错误，没有直接生成可接受签名 |
| 授权状态 | verify 成功调用 `0x149a8b94(1)` 写入 `0x80269b48`；`0x149a8b88` 返回该状态 | program 开头经 `0x149a3ab0` 读取此状态；读取也检查状态并有特定模式例外 |
| 写入与 VIP | program 还检查工作模式；Internet 授权与特定模式组合会拒绝无 VIP 写入；VIP 启用时要求 label 并调用 `0x149a8e6c` | 签名通过不等于全部分区具有写入权限 |
| 安全启动与表签名 | `0x149a8c9c` 在安全状态下调用 `0x149a4cf4` 验证表；安全状态查询失败也保持安全分支 | 非安全测试分支的存在不能证明量产 RMX3800 会跳过签名 |

设备端并不负责 HTTP 登录，因此不能仅从 loader 推出某一个服务器域名是唯一签发来源。结合官方 5.9.18.0 的 `getsigndata → RCSM sign → verify` 调用链和下一节的 OSIG 字节对照，可以确认此官方 loader **保留与 RCSM 客户端相容的设备签名协议**。没有证据把它归为“免认证 loader”，也没有证明 Anteasy 云端提供的会是同一文件或不同文件。

证据：[getsigndata 汇编](anteasy-oplustool-4.2026.009-evidence/cloud-loader/rmx3800/asm/0x149a19b8.txt)、[字段输出](anteasy-oplustool-4.2026.009-evidence/cloud-loader/rmx3800/hlil/0x149a1150.txt)、[OSIG 与 Y/N 验签分支](anteasy-oplustool-4.2026.009-evidence/cloud-loader/rmx3800/asm/0x149a1224.txt)、[签名接收](anteasy-oplustool-4.2026.009-evidence/cloud-loader/rmx3800/asm/0x149a1414.txt)、[program](anteasy-oplustool-4.2026.009-evidence/cloud-loader/rmx3800/asm/0x149a27fc.txt)、[read](anteasy-oplustool-4.2026.009-evidence/cloud-loader/rmx3800/asm/0x149a29e8.txt)、[VIP 表认证](anteasy-oplustool-4.2026.009-evidence/cloud-loader/rmx3800/asm/0x149a8cbc.txt)。

### 9.5 OSIG 输出级验证与未通过的执行范围

本节保留上一轮执行结果。SHA 失败已在第 10 节通过补齐局部运行环境解决；下表的“未执行”只描述本节对应的历史记录，不代表最新覆盖范围。

[复现脚本](../../runs/anteasy-cloud-loader/validate_loader_osig.py) 在 Unicorn 中执行 loader 的原始 AArch64 序列化指令，于 `0x149a4610` 入口停止，未执行 RSA、未填写验签结果、未改变设备授权状态。三组输入覆盖 W、R 和零值字段；它们是明确标记的合成输入，不是手机采集记录。

首个不加适配器的执行在日志传输 `0x149a4b0c` 读取未初始化的接口对象时停止。后续仅跳过 `0x149a5c34` 诊断日志调用，并记录此适配；内存复制、OSIG 组装仍运行原始代码，没有替换密码运算或返回授权成功。主机端则执行官方 `FTLibBase!0x1001d420` 到公钥查询前，沿用已核验的原生 CRT/SHA 路径，不加 API stub。

| 检查 | 结果 |
|---|---|
| loader OSIG 与独立 208 字节布局 | 3/3 逐字节相等 |
| loader OSIG 与官方 5.9.18.0 主机端 OSIG，同输入、非扩展模式、首次 Y | 3/3 逐字节相等 |
| 主机端原生 SHA-256 与 hashlib(OSIG) | 3/3 相等 |
| loader 原生 SHA 包装层单独执行 | **未通过**：密码上下文获取返回 0；包装层虽然返回，输出缓冲仍全零，与正确摘要不等 |
| loader RSA、N 重试、完整设备初始化、实际 USB/服务认证 | 未执行验证 |

loader SHA 的问题定位到 `0x149c2a04 → 0x149ad76c` 未取得上下文，随后 update/final 未产出有效摘要；这说明当前局部运行环境不足，不是原机 SHA 算法错误，也不能把“正常返回”计为密码运算成功。没有用标准摘要替换它后声称原生通过。布局中 ID 使用 8 字节后留零，Rand 为 16 字节，两个摘要各 64 字节，两个项目为 `00000`；本次没有测试任意输入长度或真实生成 Rand 的完整熵源。

证据：[未适配执行记录](anteasy-oplustool-4.2026.009-evidence/cloud-loader/osig-loader-unadapted-attempt.json)、[最终差分与 SHA 缺口记录](anteasy-oplustool-4.2026.009-evidence/cloud-loader/osig-loader-native-validation.json)、[原生 SHA 包装](anteasy-oplustool-4.2026.009-evidence/cloud-loader/rmx3800/hlil/0x149a6090.txt)、[密码上下文创建](anteasy-oplustool-4.2026.009-evidence/cloud-loader/rmx3800/hlil/0x149c2a04.txt)。

### 9.6 DIGEST 日志的含义与剩余缺口

EXE `0x1405b6580` 的 DIGEST 布尔分支输出 `OPPO DIGEST mode, skipping authorization detection`，并调用另一条主机初始化路径；它没有修改手机 loader 的 RSA 检查。`Oplus_Qualcomm!0x180033ee0` 的命令分支仍能构造 `verify value="ping" EnableVip="1"` 及 `sha256init`，并被 Digest Auth 命令适配层调用。故这条日志最多说明主机端选择了不同流程，不能作为设备端免签名证明。具体云端 sign 是否预签、绑定哪些字段、能否供当前手机接受，仍缺实际资产。

剩余缺口按优先级为：

1. **Anteasy 真实资产身份。** 未获得 RMX3800 对应的列表响应、profile/asset ID、scheme、loader/digest/sign 或其 SHA-256；无法与官方对照做二进制差分。
2. **手机身份与接受证据。** 未取得当前设备 Sahara PK hash、MSM/V3 身份，未测试 PBL 接受、Firehose ACK 或授权状态。官方包属于 RMX3800，不等于所有当前固件/DDR/安全状态都可用。
3. **密码与运行环境。** 第 10 节已补齐局部堆环境，验证原始 SHA、RSA/padding 和 Y/N 失败重试。仍缺厂商有效签名驱动的 Y/N 成功执行、完整设备初始化及公钥与服务器签发端的一一对应。
4. **受保护的 SWM bootstrap 和全链路。** 已恢复字段与前置顺序，未恢复所有绑定核心或获得有效业务会话。此次 401 不反映有权用户能看到的云端库存。
5. **写入后的结果。** 没有连接手机、上传 programmer、写分区或测试启动；下载和静态分析成果不升级为刷机成功。

本轮产物复核见 [追加验证记录](anteasy-oplustool-4.2026.009-evidence/cloud-loader/validation.json)。在上述资产与设备证据补齐之前，结论保持为：**官方 RMX3800 对照 loader 有设备签名与 VIP 门槛；Anteasy 云端候选是否改变这条链，尚不能确认。**

## 10. 官方 RMX3800 loader 的 SHA 环境修复与 RSA/Y/N 执行验证

本轮继续使用第 9.3 节的同一官方样本，未改变原始 ELF。Binary Ninja 5.2.8614 Personal GUI 导出上下文分配、堆初始化、RSA 和授权分支的汇编/HLIL；Unicorn 执行对应 AArch64 指令。**SHA 的失败来自离线入口跳过堆初始化；补齐局部环境后，12 组摘要对照通过，RSA 测试密钥用例 6/6 符合预期，固定厂商公钥下的授权拒绝用例 5/5 符合预期。** 后两项不等于厂商授权成功。

### 10.1 SHA 初始化失败的实际因果链

| 地址/对象 | Binary Ninja 证据与执行观察 |
|---|---|
| `0x149a6090 → 0x149a5f80` | SHA 包装层申请全局单实例句柄，清空 `0x80272838` 的上下文指针，再调用初始化 |
| `0x149c2a04 → 0x149ad76c` | 使用 provider `0x149d2be0` 创建上下文，首次请求 `0x78` 字节 |
| `0x149ac0d4 → 0x149abfcc → 0x1499ca74 → 0x1499c9bc` | provider 的分配回调进入通用堆，不是先失败于 SHA 压缩算法、RSA 密钥或摘要输入 |
| 描述符数组 `0x80202990`，步长 `0x80` | 默认分配器遍历 9 个槽位并跳过索引 2；每个可尝试槽位的容量字段 `+0x1c` 在新建离线 BSS 中均为 0 |
| `0x149ab6f8` | 请求 `0x78` 大于容量 0，返回空指针。执行记录实际经过 8 个槽位；`0x149c2a38` 观察到创建结果为 0 |
| `0x149a5f80 / 0x149a5fd8 / 0x149a601c` | 上层未传播 crypto init/update/final 的错误，最终仍可返回 0，但没有写出有效摘要 |

本轮把输出预填为 `A5`，未初始化时的 32 字节输出仍全为 `A5`。这进一步证明上一轮的“全零摘要”只是未写出的初始缓冲，不是 loader 实际计算出了零摘要。问题限定为当前离线调用环境，不能据此认定手机上的 SHA 实现有错。

证据：[分配器](anteasy-oplustool-4.2026.009-evidence/rmx3800-crypto/rmx3800/hlil/0x1499c9bc.txt)、[堆容量检查](anteasy-oplustool-4.2026.009-evidence/rmx3800-crypto/rmx3800/hlil/0x149ab6f8.txt)、[上下文分配](anteasy-oplustool-4.2026.009-evidence/rmx3800-crypto/rmx3800/asm/0x149ad76c.txt)、[初始化返回路径](anteasy-oplustool-4.2026.009-evidence/rmx3800-crypto/rmx3800/hlil/0x149c2a04.txt)、[上层初始化](anteasy-oplustool-4.2026.009-evidence/rmx3800-crypto/rmx3800/hlil/0x149a5f80.txt)、[final 包装](anteasy-oplustool-4.2026.009-evidence/rmx3800-crypto/rmx3800/hlil/0x149a601c.txt)。

### 10.2 补齐的环境及适配范围

离线映射保留 ELF 的原始 PT_LOAD 地址、BSS、独立栈和调用缓冲，并启用 FP/SIMD。新增 `0x32000000` 起的 2 MiB 测试堆，以原始入口 `0x1499f458(base, size, index=0)` 进入 `0x1499c7a8 → 0x149ab0bc`。堆描述符、块头、分配与释放均由 loader 自己建立和执行，没有直接写一个“分配成功”的上下文指针。

不加适配的堆初始化在 `0x149ad540` 读取未映射 MMIO `0x010c0000` 时停止。调用来自 `0x149ab2fc`，用途是给堆生成 4、2、2 字节校验字段。完整硬件随机数外设不在本轮仿真范围。

因此仅在原始堆初始化的三个确定调用点提供公开、可复现的堆元数据字节：返回位置限定为 `0x149ab1e4 / 0x149ab20c / 0x149ab218`，长度限定为 4/2/2。适配器写入相应缓冲后恢复调用者，不设置密码或授权成功返回值。它不用于 `getsigndata` 的 Rand，也不模拟真实硬件熵。RSA/授权测试另沿用第 9.5 节的诊断日志跳过；每次跳过均计数，不改返回寄存器。

环境补齐后，初始化返回 0，描述符容量为 `0x200000`。原始 SHA 上下文和 `0x6c` 字节算法状态均能分配。测试期间对代码区 `0x149997a8..0x149cd000` 做逐字节前后比较，全部保持一致；没有替换 SHA、RSA、内存分配器或授权条件。这是局部运行环境，不是完整 SoC 启动仿真。

证据：[原始堆初始化汇编](anteasy-oplustool-4.2026.009-evidence/rmx3800-crypto/rmx3800/asm/0x149ab0bc.txt)、[堆随机字段 helper](anteasy-oplustool-4.2026.009-evidence/rmx3800-crypto/rmx3800/asm/0x149ab2fc.txt)、[硬件读取路径](anteasy-oplustool-4.2026.009-evidence/rmx3800-crypto/rmx3800/asm/0x149ac010.txt)、[环境与 SHA 复现脚本](../../runs/rmx3800-loader-crypto/native_env.py)。

### 10.3 原始 SHA 的输出级验证

| 检查 | 结果 |
|---|---|
| 不初始化堆，输入 `abc`，输出预填 A5 | 复现空上下文；包装返回 0，输出未改变，明确不计通过 |
| 原始堆初始化，无硬件适配 | 在上述 MMIO 读取处停止，保留失败记录 |
| 初始化测试堆后的 SHA：长度 0、3、55、56、63、64、65、208、4096 | 9/9 与 Python `hashlib.sha256` 逐字节一致；3 字节用例为 `abc`，其他为明确合成序列 |
| 第 9.5 节 W、R、零字段的原始 208 字节 OSIG | 3/3 同时等于 hashlib 和上一轮官方 5.9.18.0 原生主机端摘要 |
| 同一堆与同一 emulator 连续执行上述 12 次 | 每次结束堆存活分配字节为 0，SHA busy=0、上下文指针=0，输出正确 |

这补齐上一轮 loader SHA 0/3 的缺口。主机端摘要复用上一轮已绑定样本的原生执行记录，本轮没有重新运行 Windows 主机代码。SHA 分块边界测试不等于覆盖任意输入长度、并发调用或所有算法后端。

完整输入摘要、输出、分配入口事件和失败/成功对照见 [SHA 环境验证 JSON](anteasy-oplustool-4.2026.009-evidence/rmx3800-crypto/sha-environment-validation.json)。

### 10.4 RSA 算法、编码和真实返回值

`0x149a4610` 从函数参数取得模数/指数十六进制串、消息和签名，构建 RSA 上下文；它先通过原始 `0x149a6090` 计算 SHA-256，再调用 `0x149c5964(ctx, 9, 32, digest, signature)`。该样本固定授权公钥位于 `0x149ce818`，为 2048 位，指数串位于 `0x149cea19`，值为 `10001`（65537）。

`0x149c5720` 建立预期的 PKCS#1 v1.5 编码块：`00 01 FF...FF 00 || DigestInfo(SHA-256, digest)`。DigestInfo 前缀实测为 `3031300d060960864801650304020105000420`。`0x149c55e8` 执行公钥运算，再在 `0x149c554c` 比较完整模数长度的编码块。RSA 有自己的静态内存池及首次初始化路径，未增加 RSA 分配/成功 stub。

为验证算法接受与拒绝语义，使用本地生成的**独立测试密钥**作为通用包装函数的正常输入参数。它不替换 `0x149a1224` 的固定厂商公钥，也不用于声称设备授权成功。

| 测试密钥用例 | 原始 RSA 返回 W0 | 包装返回 W0 | 结果 |
|---|---:|---:|---|
| 正确 PKCS#1 v1.5/SHA-256 签名 | `0` | `0` | 接受 |
| 签名不变，消息末字节改变 | `0xffffbc80` | `0xffffffff` | 拒绝 |
| 全零签名 | `0xffffbc80` | `0xffffffff` | 拒绝 |
| 用测试私钥签出 FF 填充区含 FE 的编码块 | `0xffffbc80` | `0xffffffff` | 拒绝 |
| 用测试私钥签出异常 DigestInfo 的编码块 | `0xffffbc80` | `0xffffffff` | 拒绝 |
| 全 FF、数值不小于模数的签名 | `0xffffbd7c` | `0xffffffff` | 在编码块比较前拒绝 |

6/6 符合预期。所有实际进入编码块比较的测试，恢复块逐字节等于独立 Python `pow(signature, 65537, modulus)`，预期块逐字节等于独立构造的 PKCS#1/SHA-256 编码。正确签名由 OpenSSL 生成，异常编码也经过真实测试私钥运算，未向原生函数填写验签结果。

证据：[RSA 包装汇编](anteasy-oplustool-4.2026.009-evidence/rmx3800-crypto/rmx3800/asm/0x149a4610.txt)、[PKCS#1 编码与比较](anteasy-oplustool-4.2026.009-evidence/rmx3800-crypto/rmx3800/asm/0x149c5720.txt)、[公钥运算](anteasy-oplustool-4.2026.009-evidence/rmx3800-crypto/rmx3800/asm/0x149c55e8.txt)、[固定公钥地址来源](anteasy-oplustool-4.2026.009-evidence/rmx3800-crypto/rmx3800/asm/0x149a1d18.txt)、[RSA/授权执行记录](anteasy-oplustool-4.2026.009-evidence/rmx3800-crypto/rsa-authorization-validation.json)、[复现脚本](../../runs/rmx3800-loader-crypto/validate_rsa.py)。

### 10.5 固定公钥下的 Y/N 授权状态机

以下调用从原始 `0x149a1224` 入口开始，使用 loader 自带公钥。ID/Rand/两个摘要仍为第 9.5 节明确标记的合成挑战输入，不是实机挑战。一次合法长度的失败执行顺序为：

```text
检查签名指针及长度 256
  → 组装 208 字节 OSIG，授权字段 Y
  → 原始 SHA-256 → 原始 RSA/PKCS#1 检查
  → 失败后将授权字段改为 N
  → 重新计算 SHA-256 → 再次执行原始 RSA/PKCS#1 检查
  → 两次失败：返回 0xffffffff，不写授权类型
```

| 固定公钥测试 | 实际路径 | 状态 |
|---|---|---|
| W/ASCII 字段 + 全零签名 | Y、N 均验签失败 | 返回错误；auth pass=0、auth mode=0 |
| R/ASCII 字段 + 全零签名 | Y、N 均验签失败 | 同上 |
| W/零字段 + 全零签名 | Y、N 均验签失败 | 同上 |
| W/ASCII 字段 + 独立测试密钥的正确签名 | 在厂商公钥下 Y、N 均失败 | 同上；证明测试密钥成功未被移植为厂商授权成功 |
| 同一签名但长度参数为 255 | 在入口长度检查处拒绝，0 次 RSA 调用 | 同上 |

5/5 符合预期。前四例共执行 8 次真实 RSA 检查，每次原生摘要与当次 Y/N OSIG 的独立摘要相等；所有恢复编码块均与独立模幂结果相等。每例均从未授权状态开始；本轮没有测试既有授权会话中重复 verify 的状态清理。

成功路径目前属于**静态确认**：Y 检查返回 0 时，`0x149a13dc → 0x149a8bd4(1)` 写 `0x80269b4c=1`；只有首个失败后才尝试 N，N 成功时 `0x149a13f4` 写值 2。两种成功都返回 0，随后外层 `0x149a1414` 才调用 `0x149a8b94(1)` 设置 `0x80269b48` 的总授权通过位。没有厂商有效签名，本轮未动态命中这些成功分支，也没有改跳转、公钥或返回值使其通过。

证据：[Y/N 原始控制流](anteasy-oplustool-4.2026.009-evidence/rmx3800-crypto/rmx3800/asm/0x149a1224.txt)、[授权类型写入](anteasy-oplustool-4.2026.009-evidence/rmx3800-crypto/rmx3800/asm/0x149a8bd4.txt)、[外层接收与授权通过位](anteasy-oplustool-4.2026.009-evidence/cloud-loader/rmx3800/asm/0x149a1414.txt)。

### 10.6 剩余缺口与复核产物

1. 未取得与此固定公钥、当前 OSIG 匹配的有效厂商签名；Y/N 成功分支、4096 字节真实接收链、外层成功置位尚未动态验证。
2. 未执行真实 SoC 启动、硬件熵源、真实设备字段采集、USB 或存储初始化。第 11 节已进一步恢复寄存器/存储记录到字段的路径，并在明确的环境输入下执行；这仍不等于验证了当前设备数据或真实熵。
3. 测试覆盖当前 SHA-256/RSA-2048 路径，不覆盖全部密码接口、并发/低内存故障、任意畸形输入及完整 VIP 权限表处理。
4. 未取得 Anteasy 的 RMX3800 云端资产或实机 Sahara 身份；没有新增在线登录、RCSM 签发、PBL 接受、分区写入或启动结果。

新增 [Binary Ninja 数据库](../../runs/rmx3800-loader-crypto/rmx3800-firehose-crypto.bndb)、[BN 元数据](anteasy-oplustool-4.2026.009-evidence/rmx3800-crypto/rmx3800/metadata.json)与 [本轮复核记录](anteasy-oplustool-4.2026.009-evidence/rmx3800-crypto/validation.json)独立保存。旧轮失败证据保持原字节；[证据哈希清单](anteasy-oplustool-4.2026.009-evidence/evidence-manifest.json)纳入本轮追加文件。

## 11. getsigndata 字段来源、存储记录与离线执行

本节继续分析相同的官方 RMX3800 loader，原始文件与嵌套 ELF 的 SHA-256 均未改变。Binary Ninja 导出本轮汇编、HLIL、交叉引用及常量，Unicorn 执行原始字段处理代码。**已恢复 Rand、芯片 ID、NV、DigestWrite、DigestRead 从外部输入或前序缓存到输出缓冲的路径；固定公钥下的有效存储记录验签仍缺正例。** 本轮没有连接手机，没有以合成寄存器或存储输入冒充真实设备采集。

### 11.1 入口顺序与字段消费者

`getsigndata @ 0x149a19b8` 的原始顺序是：读取芯片序列号 → 格式化 ID → 生成 Rand → 准备 DigestRead → 读取/编码 NV → 输出字段 → 设置准备状态并进入响应函数。**它没有重新计算 DigestWrite**；DigestWrite 由前序接收路径填充。

| 字段 | 输出缓冲 | 生产者与输入 | 后续消费者 |
|---|---|---|---|
| ID | `0x8025b281`，9 字节槽位 | `0x149aaf64` 读取 32 位寄存器，入口以 `%x` 格式化 | 字段输出；当前挑战 OSIG 与存储记录验签 OSIG 均复制 8 字节 |
| Rand | `0x8025b28a`，17 字节槽位 | `0x149a0d3c → 0x149a8900` 取得随机字节，再转 16 位小写十六进制 | 字段输出；当前挑战 OSIG 复制 16 字节 |
| DigestWrite | `0x8025b29b`，65 字节槽位 | 前序 `0x149a47a8 → 0x149a0ca4` 对接收的签名摘要表缓冲计算 SHA-256 | 字段输出；当前挑战 OSIG 复制 64 字节 |
| DigestRead | `0x8025b2dc`，65 字节槽位 | `0x149a0f44` 读取旧记录，验签后取其 `+0x40`；失败使用内置串 | 字段输出；当前挑战 OSIG 复制 64 字节 |
| NV | `0x8025b41d`，512 字节槽位 | `0x149a10a0 → 0x149a0fe8` 从另一存储位置读取记录，检查 OSIG 魔数并 Base64 编码前 352 字节 | 单独的 `NV:%s;` 输出；不属于此前确认的 208 字节当前挑战 OSIG |

`0x8025b280` 是挑战准备状态。主流程成功将其置 1，但不设置 `0x80269b48` 的授权通过位或 `0x80269b4c` 的授权类型。本轮正常样例在响应入口看到 `ACK` 参数 1、准备状态 1，而两个授权字段仍为 0。

证据：[入口汇编](anteasy-oplustool-4.2026.009-evidence/rmx3800-getsigndata/rmx3800/asm/0x149a19b8.txt)、[字段输出汇编](anteasy-oplustool-4.2026.009-evidence/rmx3800-getsigndata/rmx3800/asm/0x149a1150.txt)、[字段生产者/消费者交叉引用](anteasy-oplustool-4.2026.009-evidence/rmx3800-getsigndata/rmx3800/refs-0x8025b29b-0x8025b2dc-0x8025b41d-0x8025b28a-0x149a0dac-0x149a19b8.json)、[当前挑战序列化](anteasy-oplustool-4.2026.009-evidence/cloud-loader/rmx3800/asm/0x149a1224.txt)。

### 11.2 Rand：寄存器轮询、异或和 64 位编码

原始调用链为 `0x149a0d3c → 0x149a8900 → 0x149ac010 → 0x149ad4ec`。provider 可选择回调；本 ELF 默认路径进入硬件寄存器读取。此次采用的明确测试环境中，版本寄存器 `0x010c0000=0x30000`，通道选择为 3，状态寄存器为 `0x010c3004`，数据寄存器为 `0x010c3000`。

在已执行的路径中，每产生一个 32 位结果：先等待状态 bit 0 为 1，读取第一个来源字并要求其低 16 位非零；再等待就绪，读取第二个来源字并要求数值至少 `0x10000`；两个字异或，将结果按小端写入目的缓冲。请求的 16 字节需要 4 对、共 8 个被接受的来源字。未满足条件时由原始代码重新读取；本轮未给 RNG 函数直接填写成功返回值。

`0x149a0d3c` 随后只加载目的缓冲的前 8 字节，交给 `0x149a7fd8` 编码为 16 位小写十六进制，并补 NUL。因此当前实现的输出仅保留前两次异或结果的信息：

```text
w0 = source[0] XOR source[1]
w1 = source[2] XOR source[3]
Rand = hex16((uint64(w1) << 32) | w0)
```

示例输入得到 `w0=0x95511559`、`w1=0xbb3333bb`，原始输出为 `bb3333bb95511559`。改变后 8 个生成字节对应的来源字，Rand 不变；改变前半部分则改变输出。两个符合读取条件的来源字相等时，异或结果可以为零，原始代码仍输出零字符。这是数据流观察，不是对真实硬件熵质量或可预测性的结论。

6 组结果符合预期：正常、仅改变后半部、改变前半部、异或为零、无效来源字重试，以及状态始终未就绪。重试用例实际消费 10 个来源字；始终未就绪用例在 5000 条指令预算处停止，未返回成功、未读取数据字。没有把这个预算停止解释为 loader 自带超时。

证据：[Rand 包装](anteasy-oplustool-4.2026.009-evidence/rmx3800-getsigndata/rmx3800/asm/0x149a0d3c.txt)、[原始寄存器路径](anteasy-oplustool-4.2026.009-evidence/rmx3800-getsigndata/rmx3800/asm/0x149ac010.txt)、[64 位编码](anteasy-oplustool-4.2026.009-evidence/rmx3800-getsigndata/rmx3800/asm/0x149a7fd8.txt)、[Rand 执行对照](anteasy-oplustool-4.2026.009-evidence/rmx3800-getsigndata/rand-validation.json)。

### 11.3 芯片 ID：直接寄存器读取与变长十六进制

`0x149aaf64` 在 `0x149aaf7c` 直接读取 `0x221c8610` 的 32 位值，写给调用者；样本日志称其为 `SM8650 oplus_get_soc_serial_number`。入口拒绝 `0` 和 `0xffffffff`，其余值用常量 **`%x`** 写入容量为 9 的缓冲，使用小写、不补前导零。

原始执行确认 `1 → "1"`、`0xabcdef → "abcdef"`、`0x80000000 → "80000000"`、`0xfffffffe → "fffffffe"`。两种无效值在完整入口测试中于生成 Rand 前被拒绝，准备状态/响应参数均为 0。这里的寄存器输入是明确的测试值；未读取当前手机的序列号，也未用机型目录值 23631 代替它。

格式化字符串长度可小于 8，后续 OSIG 仍复制该槽位的 8 字节。本轮正常新建 BSS 的未写部分为零；没有把变长文本误写为固定 8 位补零格式，也没有声称覆盖了所有可能的历史缓冲状态。

证据：[寄存器读取](anteasy-oplustool-4.2026.009-evidence/rmx3800-getsigndata/rmx3800/asm/0x149aaf64.txt)、[常量读回](anteasy-oplustool-4.2026.009-evidence/rmx3800-getsigndata/constants-readback.json)、[边界与寄存器观察](anteasy-oplustool-4.2026.009-evidence/rmx3800-getsigndata/source-boundary-validation.json)。

### 11.4 DigestWrite：前序签名摘要表的完整缓冲摘要

`0x149a47a8` 在相关表状态未准备好时取得容量 `0x9000` 的缓冲，经传输函数接收数据并取得实际长度；随后调用 `0x149a8c9c`。安全状态下后者进入签名表认证，非安全测试路径另行解析表头。仅当此检查成功，才以**原接收指针和实际接收长度**调用 `0x149a0ca4`，然后设置表已准备状态。

`0x149a0ca4` 清空 65 字节输出 → 原始 SHA-256 → `%02X` 编码 32 字节 → 转小写。其输入边界是传输缓冲及实际长度，不能根据名称将它解释为固件所有分区、loader 本身或只抽取表中某个哈希的摘要。

本轮使用此前从同一官方包取得的 `DigestsToSign_nv00000000.bin.mbn`，33144 字节。直接以该文件作为原始摘要函数输入，得到：

```text
9211d9b8d2d77a5d94f9f4484918a16097ad4cf78e436ab1e6ff27ac0838f075
```

结果与独立文件 SHA-256 一致。这里验证的是摘要生产函数，**未执行该文件上游的真实安全世界签名表认证或 USB 接收**。直接执行 getsigndata 而不先填入 DigestWrite 的用例，输出仍为空，证明此入口读取已有缓存，不自行补算。

证据：[前序接收与检查顺序](anteasy-oplustool-4.2026.009-evidence/rmx3800-getsigndata/rmx3800/asm/0x149a47a8.txt)、[摘要生产函数](anteasy-oplustool-4.2026.009-evidence/rmx3800-getsigndata/rmx3800/asm/0x149a0ca4.txt)、[签名表检查](anteasy-oplustool-4.2026.009-evidence/rmx3800-getsigndata/rmx3800/asm/0x149a8c9c.txt)、[官方文件输入验证](anteasy-oplustool-4.2026.009-evidence/rmx3800-getsigndata/digest-write-validation.json)、[来源哈希](anteasy-oplustool-4.2026.009-evidence/rmx3800-getsigndata/source-identities.json)。

### 11.5 DigestRead：固定位置的签名旧记录

`0x149a0f44` 先调用安全状态查询 `0x149a4c9c`。原始安全监控调用号为 `0x02000102`；查询返回错误时按安全状态处理。非安全状态直接使用默认串；安全状态尝试分配传输缓冲，并由 `0x149a895c` 选择以下位置：

| 存储类型 | slot | unit | 起始 LBA | 读取扇区数 |
|---|---:|---:|---:|---:|
| type 3（UFS 路径） | 0 | 3 | `0x260` | 1 |
| 其他类型分支（eMMC 描述符 type 1） | 0 | 2 | `0x26` | 1 |

读操作为 `0x149a44c0` 调用存储 open/read/close。UFS 测试扇区为 4096 字节，eMMC 为 512 字节，但传入记录验签函数 `0x149a0dac` 的逻辑长度均固定为 `0x200`。

记录验签重新建立 208 字节 OSIG：固定 `OSIG/0/W/QCOM/SM8650` 和项目 `00000`；授权字段取记录 `+0x08`，ID 取**当前芯片 ID**，Rand 取**记录 `+0x30`**，两个摘要分别取记录 `+0x40/+0x80`；签名取记录 `+0x100` 的 256 字节，公钥仍为 `0x149ce818`。因此这里并不使用本次新生成的 Rand 或 DigestWrite 缓存。

本轮 5 次实际进入此验签路径的完整用例中，组装的 OSIG 均与独立布局相等，原生 SHA 均与独立摘要相等，合成记录的签名均真实失败。失败后使用内置 64 字节默认串：

```text
0123456789012345678901234567890101234567890123456789012345678901
```

**成功后复制记录 `+0x40` 的 64 字节到 DigestRead，仍仅有静态证据。** 不能因目标字段叫 DigestRead，就误认为来源是记录 `+0x80`。签名失败、分配失败、存储读取失败及非安全状态都会选择默认串；该 helper 最终返回 0，默认值本身不会让 getsigndata 失败。

证据：[位置选择](anteasy-oplustool-4.2026.009-evidence/rmx3800-getsigndata/rmx3800/asm/0x149a895c.txt)、[读取与默认路径](anteasy-oplustool-4.2026.009-evidence/rmx3800-getsigndata/rmx3800/asm/0x149a0f44.txt)、[记录重建与验签](anteasy-oplustool-4.2026.009-evidence/rmx3800-getsigndata/rmx3800/asm/0x149a0dac.txt)、[安全监控调用](anteasy-oplustool-4.2026.009-evidence/rmx3800-getsigndata/rmx3800/asm/0x149c2988.txt)、[五组记录数据流对照](anteasy-oplustool-4.2026.009-evidence/rmx3800-getsigndata/record-dataflow-validation.json)。

### 11.6 NV：oplusreserve1 定位与 352 字节编码

`0x149a10a0` 分配缓冲，`0x149a8b20 / 0x149a8998` 建立存储描述符并查找 `oplusreserve1`。查找路径 `0x149aa980 → 0x149aa6c4` 从 LBA 0 读取 UFS 的 6 个或 eMMC 的 32 个扇区，检查下一扇区的 `EFI PART`，按表中 entry LBA、entry 数量和 128 字节记录遍历名称，取得分区起点和长度。

| 存储路径 | slot/unit | 查到分区时的 NV LBA | 零输出默认路径的 NV LBA | NV 读取长度 |
|---|---|---|---|---|
| UFS/type 3 | 0 / 5 | `partition_start + 0x48d` | `0x4ad` | 1 个扇区 |
| eMMC/type 1 | 0 / 0 | `partition_start + 0x2344` | `0x2345` | 1 个扇区 |

同一官方包的 `rawprogram5.xml` 声明 `oplusreserve1` 在物理分区 5、4096 字节扇区、起点 32；`32 + 0x48d = 0x4ad` 与 UFS 默认常量相符。这是包声明与代码的对照，不能替代当前手机 GPT。

取得记录后，`0x149a0fe8` 只检查前四字节为 `OSIG`，清空 512 字节 NV 输出缓冲，然后调用原始 Base64 编码器处理**前 `0x160=352` 字节**，得到 472 个 Base64 字符。此函数没有执行该记录的 RSA 验证。4 组逐字节测试确认标准 Base64 输出、最后纳入字节的影响、首个排除字节不影响输出，以及错误魔数拒绝。

错误处理有两个层次：NV 初始缓冲分配失败返回 `0xffffffff`，入口据此失败；若已分配缓冲，后续读取或记录格式错误会清空 NV，但 helper 仍返回 0，入口可以继续输出并准备挑战。本轮存储读取失败/错误魔数用例均实际观察到 NV 为空、准备状态及响应参数仍为 1。

另有一个已执行确认的环境敏感点：`0x149a8998` 未先初始化给 GPT helper 的两个栈输出，也未使用其返回值决定是否采用输出；查找未命中时，它们可以保持原值。UFS/eMMC 各测试“找到分区、未找到且初始值为零、未找到且初始值非零”，6/6 与原始指令行为相符。非零测试中旧起点 `0x1234` 被继续加偏移，故**不能把所有 GPT 查找失败都概括成稳定使用默认 LBA**。这不是当前手机发生过错误定位的证据，正常包分区匹配仍需真实 GPT 核验。

证据：[NV 位置选择](anteasy-oplustool-4.2026.009-evidence/rmx3800-getsigndata/rmx3800/asm/0x149a8998.txt)、[GPT 读取](anteasy-oplustool-4.2026.009-evidence/rmx3800-getsigndata/rmx3800/asm/0x149aa980.txt)、[GPT 解析](anteasy-oplustool-4.2026.009-evidence/rmx3800-getsigndata/rmx3800/asm/0x149aa6c4.txt)、[NV 编码](anteasy-oplustool-4.2026.009-evidence/rmx3800-getsigndata/rmx3800/asm/0x149a0fe8.txt)、[NV 错误传播](anteasy-oplustool-4.2026.009-evidence/rmx3800-getsigndata/rmx3800/asm/0x149a10a0.txt)、[定位验证](anteasy-oplustool-4.2026.009-evidence/rmx3800-getsigndata/storage-selector-validation.json)、[编码验证](anteasy-oplustool-4.2026.009-evidence/rmx3800-getsigndata/nv-encoding-validation.json)。

### 11.7 本轮运行环境与验证范围

新增环境在第 10 节 ELF/BSS/栈映射上补齐如下依赖：

| 环节 | 本轮执行方式与限制 |
|---|---|
| 通用堆 | 仍调用原始初始化；现在通过 MMIO 输入使原始 RNG 执行，未再跳过堆校验字段 helper。初始化实际消耗 6 个来源字 |
| 传输缓冲池 | 调用原始 `0x149a4fa0`；原始分配器返回 `0x80000000 / 0x80100000` 两个 1 MiB 缓冲，第三次分配失败，原始释放后状态归零 |
| 存储选择 | 通过原始 `0x149a8be0` 写入明确的 UFS/type 3 或 eMMC/type 1 测试描述符 |
| 芯片 ID、随机源 | 提供明确的只读寄存器输入；格式化、轮询、拒绝重试、异或和编码全部执行原始指令 |
| 安全监控 | 仅在 `SMC #0` 的已知查询 `0x02000102` 提供模拟的安全状态或查询错误；未模拟签名认证成功，未替换 RSA |
| 存储 | 在 `0x149a617c` open、`0x149a6370` read、`0x149a6300` close 边界提供有记录的只读块设备输入/错误；其上层选择、分配、GPT 解析、OSIG 重建和 Base64 均原生执行 |
| 日志与响应 | 记录并跳过诊断日志，不设置返回值；完整入口测试在 `0x149a84f0` 响应入口停止，观察 ACK 参数。没有执行真实 USB/XML 发送 |

不提供芯片寄存器映射时，入口真实停止于读取 `0x221c8610`。提供寄存器和内存环境、但不提供存储边界时，执行在 `0x149ca1c8` 读取 `0x01fc8004` 时停止。这些失败记录明确了当前适配边界，不能把上层用例通过写成物理 UFS/eMMC 已模拟成功。

| 验证组 | 结果 |
|---|---|
| Rand 及来源重试/未就绪 | 6/6 符合预期，包含预算内未完成的负例 |
| 芯片 ID 编码边界 | 4/4 符合预期；0/FFFFFFFF 拒绝另在完整入口用例覆盖 |
| NV Base64 与长度/魔数边界 | 4/4 符合预期 |
| UFS/eMMC 的 GPT 命中与初始输出状态 | 6/6 符合预期 |
| 官方签名摘要表的原始 DigestWrite 计算 | 1/1 与文件 SHA-256 相等；未声称上游签名表被接受 |
| getsigndata 到响应入口 | 10/10 符合预期：UFS、eMMC、安全查询失败、非安全输入、读取失败、错误 NV 魔数、ID=0、ID=FFFFFFFF、未初始化 DigestWrite、缓冲耗尽 |
| 旧记录验签的输入/摘要关系 | 实际进入的 5 例均匹配独立 OSIG 布局及 SHA；签名均失败，未提供厂商正例 |
| 授权与代码完整性 | 所有完整入口用例授权位/类型保持 0；所检查原始代码区前后逐字节不变；正常用例的两个传输缓冲均释放 |

复现：[环境实现](../../runs/rmx3800-getsigndata/signdata_env.py)、[主验证脚本](../../runs/rmx3800-getsigndata/validate_dataflow.py)、[来源边界/记录关系验证](../../runs/rmx3800-getsigndata/validate_edgepaths.py)。证据：[原始初始化验证](anteasy-oplustool-4.2026.009-evidence/rmx3800-getsigndata/native-initialization-validation.json)、[完整入口执行记录](anteasy-oplustool-4.2026.009-evidence/rmx3800-getsigndata/getsigndata-integrated-validation.json)。

### 11.8 剩余缺口

1. 缺当前手机的真实寄存器、随机源、GPT、签名旧记录及 NV 数据；本轮恢复的是输入到字段的逻辑，不是采集当前设备信息。
2. Rand 的已执行路径受上述版本、通道和 provider 配置限定；其他硬件版本、回调模式、完整时钟/熵源启动与真实熵质量仍未验证。
3. 缺固定厂商公钥下的有效旧记录签名，DigestRead 成功复制分支仍未动态命中；Y/N 授权成功分支也仍缺正例。不能用修改公钥或指定成功返回值填补此缺口。
4. 缺真实签名摘要表的安全世界认证、完整 USB 收发、物理存储驱动、重复会话的所有缓存状态和实机接受证据；本轮没有刷写分区。
5. GPT 未命中时的栈输出依赖已在受控条件复现，但没有证明当前 RMX3800 的实际启动状态或分区布局会触发它。

本轮另存 [Binary Ninja 数据库](../../runs/rmx3800-getsigndata/rmx3800-getsigndata.bndb)、[BN 元数据](anteasy-oplustool-4.2026.009-evidence/rmx3800-getsigndata/rmx3800/metadata.json)和[产物复核记录](anteasy-oplustool-4.2026.009-evidence/rmx3800-getsigndata/validation.json)。此前 287 项证据的内容保持不变，本轮新增文件另行纳入总清单。

## 12. 签名摘要表的安全世界边界与真实旧记录搜索

本节继续分析同一官方 loader，原始 SHA-256 仍为 `daaddec2d87fa2f30a527f0e3d656044ef9e2fdece8163afde856cb9505e34ea`。Binary Ninja GUI 5.2.8614 导出原始 AArch64 指令、HLIL 和交叉引用；离线执行保持原始代码不变。新增结论是：**官方表的文件内密码关系可以离线验证；设备安全世界是否接受该表，以及 DigestRead 的 RSA 正例，仍是两个未闭合的环节。**

### 12.1 安全表认证调用与错误传播

入口 `0x149a47a8` 接收表后调用 `0x149a8c9c`。后者查询安全状态；安全路径调用 `0x149a4cf4`，再进入 `0x149c2988` 的 `SMC #0`。安全查询错误仍按安全路径处理，见第 11 节。

`0x149a4cf4` 检查输入地址按 4096 字节对齐，然后按 `CTR_EL0` 的缓存行大小执行 `DC CIVAC`，并经 `DSB SY / ISB` 完成屏障。它清零 24 字节结果区，按以下寄存器提交请求：

| 寄存器 | 原始调用值 | 本轮含义边界 |
|---|---|---|
| X0 | `0x02000201` | 认证调用号；尚未定位接收端的确切 handler |
| X1 | `0x14` | 参数描述值；此处仅记录代码值 |
| X2 / X3 | 表地址 / 32 位长度零扩展 | 官方样本长度 `0x8178=33144` |
| X4 | `0x1a` | 与表内元数据的 image ID 值相等 |
| X5 / X6 | 0 / 0 | 原始固定参数 |
| X7 | 本地 3 个 uint64 结果槽地址 | 包装器恢复该指针，将返回 X1/X2/X3 写入这三个槽 |

SMC 返回 X0=1 时，原始汇编回到 SMC 指令继续调用，未重新构造初次寄存器。只有返回 W0=0 且结果槽 0 的 64 位值为零，包装器才返回 1，并把结果槽 1 作为载荷指针、槽 2 的低 32 位作为载荷长度交给上层。**此接受分支目前为静态证据，没有注入成功结果。**

返回错误时，包装器返回 0、输出指针回填原始输入、输出长度清零。地址不对齐是更早的分支，输出参数保持原值。两种行为不能合并描述。外层 `0x149a8c9c` 在认证失败时返回 7；实测此路径的 `0x80269b70` 仍为原始输入指针，不能用“指针非零”判断通过。

认证通过后的静态路径还要求：载荷长度大于 31、魔数 `0x7cef0312`、`code_size + 32 == 返回长度`、`record_count × record_size == code_size`。随后才建立条目指针。入口仅在这些步骤返回成功后，调用 `0x149a0ca4` 对**整个收到的签名表**计算 SHA-256，形成 DigestWrite，并设置表状态。

离线使用未修改的官方表，实际执行到 SMC 边界；缓存行覆盖 `0x31000000–0x31008140`，共 518 次，所用仿真 CPU 的行大小为 64 字节。另以明确的外部失败输入验证非零 SMC 返回、零 SMC 返回但非零服务状态，以及一次 X0=1 后失败；均被原始代码拒绝。没有模拟真实缓存一致性、EL3 调度或签名认证成功。

证据：[认证包装器](anteasy-oplustool-4.2026.009-evidence/rmx3800-secure-auth/rmx3800/asm/0x149a4cf4.txt)、[SMC ABI](anteasy-oplustool-4.2026.009-evidence/rmx3800-secure-auth/rmx3800/asm/0x149c2988.txt)、[载荷检查](anteasy-oplustool-4.2026.009-evidence/rmx3800-secure-auth/rmx3800/asm/0x149a8c9c.txt)、[表接收与 DigestWrite 顺序](anteasy-oplustool-4.2026.009-evidence/rmx3800-secure-auth/rmx3800/asm/0x149a47a8.txt)、[原始边界执行](anteasy-oplustool-4.2026.009-evidence/rmx3800-secure-auth/boundary-validation.json)。

### 12.2 官方签名表：ECDSA / SHA-384 离线验证

本轮检查此前完整取得的 `IMAGES/`、`GKI_IMAGES/` 下六个 `DigestsToSign_nv*.bin.mbn`，共四种不同 SHA-256。它们均为 33144 字节、三个 program header 的 ELF。不能把这类文件中的签名拿来充当 256 字节 OSIG/RSA 响应。

| 文件范围或字段 | 六个成员的共同布局与验证 |
|---|---|
| `0x0000–0x00e8` | ELF 头与 program headers；其 SHA-384 匹配哈希段中的第一项 |
| `0x00e8–0x6594` | 25772 字节 SVIP 载荷；其 SHA-384 匹配第二项 |
| `0x7000` | 版本 7 的签名元数据段；首十个 uint32 与验证脚本留存 |
| `0x7030` | 元数据中的 image ID 为 `0x1a`，与 loader 的 SMC X4 一致 |
| `0x7120–0x71b0` | 三项、每项 48 字节；前两项为上述 SHA-384，第三项为零 |
| `0x7000–0x71b0` | ECDSA 签名覆盖的实际字节区间，右端不包含 |
| `0x71b0–0x7218` | 104 字节签名槽；DER 实际为 102、103 或 104 字节，较短时尾部为零填充 |
| `0x7218` 起 | 叶证书、CA 证书、根证书的 DER 对象 |

叶公钥为 `secp384r1 / P-384`。六个成员的原始 ECDSA/SHA-384 验签全部通过；分别翻转已签名元数据的一个位，六次全部拒绝。两类段摘要共 12 项全部匹配。DER 长度必须按实际对象读取：直接把尾部填充算进 DER 会导致较短签名的 OpenSSL 校验失败。

证书路径为 `OPLUS SM8650 Attestation → OPLUS Attestation CA → OPLUS ROOT CA 1`。六次链签名验证均通过，并检查了根自签名；**信任锚来自同一个包，时间检查明确关闭**。根 DER 的 SHA-256 为 `c785747dff01f3f83db665c159d81d1a5977a6a7b1e2e65bc84afb50ad21cd83`。这些结果证明文件、签名和内嵌证书之间的密码一致性，不证明这张根已烧录到手机，也不证明 image ID、版本/防回滚及设备策略被接受。

这里有三种不同摘要关系：签名表内的段哈希与 ECDSA 使用 SHA-384；getsigndata 的 DigestWrite 对整个表使用 SHA-256；设备 OSIG 使用 RSA-2048/PKCS#1 v1.5/SHA-256。不能因它们都来自同一刷写流程就合并成一种认证。

证据：[六表密码验证、证书信息和篡改负例](anteasy-oplustool-4.2026.009-evidence/rmx3800-secure-auth/signed-table-crypto-validation.json)。复现：[验证脚本](../../runs/rmx3800-secure-auth/verify_tables.py)。

### 12.3 安全世界接收端的定位边界

除 Firehose 外，本轮在 Binary Ninja 中分析 programmer 的 `0x31f70` 嵌套 AArch64 ELF，并从同一官方包补取 `tz.mbn`、`xbl_s.melf`、`xbl_config.elf` 等成员。嵌套引导 ELF 有 4343 个已识别函数；`tz.mbn` 补充入口分析后有 4496 个。函数数量只描述数据库，不代表路径覆盖。

嵌套引导代码的 `0x14842308 → 0x14860e64 → 0x14889014` 可见另一条启动镜像元数据认证调用，并存在 EL3 初始化、异常向量设置及共享区交互。`tz.mbn` 也含 EL3 入口和 QSEE 代码。但目前没有建立“Firehose 的 `0x02000201` → 该镜像内确定接收函数”的调用链，不能把普通启动镜像的认证函数直接命名为本次 SMC 的服务实现。安全状态、根选择、熔丝、回滚检查和验证结果来源仍未在原始安全世界代码中执行闭合。

证据：[嵌套引导认证调用](anteasy-oplustool-4.2026.009-evidence/rmx3800-secure-auth/embedded/asm/0x14842308.txt)、[后续调用](anteasy-oplustool-4.2026.009-evidence/rmx3800-secure-auth/embedded/asm/0x14889014.txt)、[系统指令定位清单](anteasy-oplustool-4.2026.009-evidence/rmx3800-secure-auth/secure-instruction-inventory.json)、[包内嵌套 ELF](anteasy-oplustool-4.2026.009-evidence/rmx3800-secure-auth/boot-nested-elf.json)。

### 12.4 DigestRead 旧记录来自 dinfo；writever 是写入者

官方包的 `gpt_main3.bin` 实际 GPT 条目与 `rawprogram3.xml` 均表明：LUN 3 的 `dinfo` 从 LBA 608=`0x260` 开始，长度 4 个 4096 字节扇区。这正好对应第 11 节的 UFS DigestRead 位置。GPT 头及分区项数组的 CRC 均通过。**它不是 NV 所在的 LUN 5 / oplusreserve1。**

命令表将 `writever` 映射到 `0x149a1b10`。该函数首先读取授权通过位；为零则返回 `0x90000001`，本轮实际观察到没有存储调用。静态通过路径调用 `0x149a15e0`，使用与 DigestRead 相同的位置选择器 `0x149a895c`，经 `0x149a14f0` 组装后调用存储写接口。`verify` 成功 helper 先把收到的 256 字节签名缓存到 `0x8025b31d`；记录持久化是后续 `writever` 命令的动作。

原生记录构造器先清零 4096 字节，再填写如下字段：

| 记录偏移 | 原生写入内容 |
|---|---|
| `+0x08` | 两字节 `Y\0` 或 `N\0`；授权类型值等于 1 时为 Y，否则为 N |
| `+0x30` | 已缓存 Rand 的 16 字节 |
| `+0x40` | 已缓存 DigestWrite 的 64 字节 |
| `+0x80` | 已缓存 DigestRead 的 64 字节 |
| `+0x100` | 已缓存原始签名的 256 字节 |
| 其余位置 | 零；包括开头、设备 ID、OSIG 固定文字对应位置 |

**dinfo 记录自身没有 `OSIG` 魔数，也不保存当前 ID。** DigestRead 的 `0x149a0dac` 读取后才重建 208 字节 OSIG，并将当前芯片 ID 加入签名消息。这解释了记录与设备的绑定，也说明仅搜索 `OSIG` 会漏掉正确记录。NV 的前四字节 OSIG 检查属于另一格式，不能套用到 dinfo。

以公开合成字段直接执行记录构造器，Y/N 两个序列化用例均与独立 4096 字节布局逐字节一致，授权通过位始终为零，未调用存储写接口。这只是 helper 的格式验证，不是 verify 成功或 writever 获准。有效厂商签名的成功复制分支仍未执行。

证据：[writever 门槛](anteasy-oplustool-4.2026.009-evidence/rmx3800-secure-auth/rmx3800/asm/0x149a1b10.txt)、[记录构造](anteasy-oplustool-4.2026.009-evidence/rmx3800-secure-auth/rmx3800/asm/0x149a14f0.txt)、[写入路径](anteasy-oplustool-4.2026.009-evidence/rmx3800-secure-auth/rmx3800/asm/0x149a15e0.txt)、[GPT/XML 对照](anteasy-oplustool-4.2026.009-evidence/rmx3800-secure-auth/record-search-and-partitions.json)。

### 12.5 从官方包寻找可用记录的结果

本轮通过严格 HTTP Range 从[固定 realme 官方服务包](https://rms11.realme.net/sw/RMX3800domestic_11_16.0.5.740CN01_2026041411410326.zip)补取 11 个成员。每个完整成员经 ZIP CRC 校验，并记录范围、ETag、大小与 SHA-256；没有下载和验证整份 ZIP。新增成员含两个 reserve 镜像、emmc_fw、engineering_cdt、LUN 3/5 GPT、TZ/XBL 及安全固件。

连同此前取得的成员，按清单扫描了 68 个文件、64 个不同 SHA-256，同时搜索 OSIG 和**不要求魔数的 dinfo 字段布局**。有一个宽松字段形状命中：外层 loader 文件偏移 `0x147345`，对应 BN 地址 `0x149ce7f5`。BN 读取与文件逐字节相等；该区域是 `%c\0OSIG\0Y\0firehose work in %s mode\0` 及后续 ASCII RSA 公钥，且不满足原生记录的零填充区。它不是持久签名记录。符合原生构造布局的候选为零。

包结构进一步限定了可取得的数据：

- 完整成员目录没有名称含 `dinfo` 的镜像；`rawprogram3.xml` 的 dinfo 项 `filename=""`，未提供该分区的输入文件。不能由此推断当前设备 dinfo 为空或必然被擦除。
- `emmc_fw.bin` 对应 oplusreserve1，其文件只有 `0x400000` 字节；NV 的分区内偏移为 `0x48d000`，已超出文件长度。XML 声明的分区范围更大，不表示文件隐含这些字节。
- oplusreserve2 为稀疏镜像：逻辑长度 256 MiB，仅四个 RAW 块合计 57344 字节有内容，四个 DONT_CARE 区不提供确定设备数据。RAW 内容中没有记录候选；没有把空洞补零后冒充实机内容。
- 六个真实签名摘要表及包清单 `.sig` 属于其他签名对象；它们没有提供与当前 ID、旧 Rand、读写摘要对应的 256 字节 RSA 签名。

所以，本轮**没有取得可用于 DigestRead 成功分支的真实记录**。该结论只适用于列出的成员和搜索范围，不是“整包任何位置都绝无记录”的证明，也不是对所有 RMX3800 历史包的结论。

证据：[第一批下载](anteasy-oplustool-4.2026.009-evidence/rmx3800-secure-auth/package-acquisition.json)、[引导成员下载](anteasy-oplustool-4.2026.009-evidence/rmx3800-secure-auth/boot-package-acquisition.json)、[逐文件扫描与稀疏覆盖](anteasy-oplustool-4.2026.009-evidence/rmx3800-secure-auth/record-search-and-partitions.json)、[伪命中定位](anteasy-oplustool-4.2026.009-evidence/rmx3800-secure-auth/record-candidate-classification.json)。

### 12.6 本轮验证与剩余缺口

| 验证组 | 结果与适用边界 |
|---|---|
| 表认证包装器 | 5/5：实际到达 SMC、地址拒绝、两类错误、一次中断后失败 |
| 安全表外层错误传播 | 2/2 返回 7；明确模拟安全查询和失败状态，没有模拟认证成功 |
| dinfo 格式与 writever 门槛 | 2/2 原生序列化相等；1/1 未授权调用拒绝、零存储调用 |
| 官方表签名与摘要 | 6/6 ECDSA 正例，6/6 单比特翻转负例，12/12 段摘要匹配 |
| 证书链 | 6/6 链签名通过；包内自带根，关闭时间检查，非设备信任验证 |
| 包数据与 GPT | 11 个新成员 CRC 通过；两份 GPT 头及项数组 CRC 通过 |
| DigestRead 厂商 RSA 正例 | 未取得；未通过修改公钥、签名或成功返回值构造正例 |

剩余工作有明确依赖：需要先定位 EDL 运行时实际承接 `0x02000201` 的安全世界映像/handler，恢复设备信任根、image ID 和回滚策略，才能将文件内验签提升为原始服务验签。要验证 DigestRead 成功分支，则需要合法取得的 dinfo 记录及其对应芯片 ID；公开表证书和表签名不能替代它们。真实 USB、物理存储、云端签名服务、设备 ACK/读回仍未执行，本轮没有操作手机。

复现脚本：[边界与格式验证](../../runs/rmx3800-secure-auth/validate_boundary.py)、[官方表密码验证](../../runs/rmx3800-secure-auth/verify_tables.py)、[记录/GPT 搜索](../../runs/rmx3800-secure-auth/scan_records.py)。另存 [Firehose BNDB](../../runs/rmx3800-secure-auth/rmx3800-secure-auth.bndb)、[嵌套引导 BNDB](../../runs/rmx3800-secure-auth/embedded-boot.bndb)、[TZ BNDB](../../runs/rmx3800-secure-auth/tz.bndb)。[产物检查](anteasy-oplustool-4.2026.009-evidence/rmx3800-secure-auth/validation.json)核验本轮脚本、数据库和样本；此前 423 项证据内容保持不变，新证据追加到总清单。
