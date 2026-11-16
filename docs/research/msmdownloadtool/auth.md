# MSMDownloadTool 认证协议

2026-11-15 补充：第 9 节记录官方 OplusFlashTool 5.9.18.0；第 10 节记录第三方 Anteasy OplusTool 4.2026.009。此前各节仍按原 MSM 样本限定。

本文从既有 Binary Ninja 分析报告和执行证据中提取认证数据流。覆盖五个主 EXE 样本；结论按版本限定。本文中的“静态确认”指客户端指令、调用参数或配置已确认，“执行验证”指指定输入下运行了目标机器码并比较输出。两者均不代表真实服务或 programmer 已接受请求。

设备传输和调用顺序见 [刷写协议](flash.md)，包内项目、型号和镜像字段见 [OTA 解包解析](ota.md)。本次整理没有新增联网登录、许可证签发或设备实验。

## 1. 样本及协议覆盖

| 主 EXE 版本 | 包来源 | 本文覆盖 |
|---|---|---|
| 3.0.2.0 | OnePlus 3 / OxygenOS 3.1.2 | 本地模式及密码门槛；已审计路径未恢复出在线账号链 |
| 4.0.28.0 | OnePlus 6 / OxygenOS 5.1.5 | 内网探测、明文配置分支及设备身份检查 |
| 5.0.15.0 | OnePlus 7T / Global 10.0.4 | 许可证 UI、申请文件、demacia/setprojmodel 调用边界 |
| 5.1.75.0 | OnePlus 8 / Global 11.0.4.4 | 客服、MES/PCBA、许可证原生核心、三类设备 token |
| 5.1.77.0 | OnePlus 9 Pro / Global 11.2.2.2 | 客服及 MES/PCBA 客户端、三类 token 与 5.1.75.0 的输出差分 |

以下 SHA-256 绑定的是原始主 EXE。恢复镜像中的地址采用对应版本的 VA，ImageBase 为 `0x400000`；不能跨版本直接复用地址或对象偏移。

| 版本 | 原始主 EXE SHA-256 |
|---|---|
| 3.0.2.0 | `204b4d16aae52fb7940613f5934a710a5389257b3b44781ede7a01e35d5ca92e` |
| 4.0.28.0 | `ea4f8ae38e2feb09def08ff5be35c563376b1773cfc64b95b1128e994e0387c8` |
| 5.0.15.0 | `ae631944f471dca00c83a91332e8e16fb8a17139a986255812ff5620ef76ee3d` |
| 5.1.75.0 | `0ac4005251f060cc759b251fcbb52a07d146c54bd8e4f4eb62092c70f28dc0b6` |
| 5.1.77.0 | `4dc3c885b3d099568f7a7e8f05fa0cdd676a07ab4102ce145c15eacefda90246` |

来源和获取记录分别见 [4.0.28 报告 §2](../msmdownloadtool-analysis.md#v4028)、[5.1.75 报告 §2](../msmdownloadtool-analysis.md#v5175)、[多版本报告 §2](../msmdownloadtool-analysis.md#multi)。社区 ZIP 的文件身份、CRC 和哈希不等于厂商签名证明。Win7/factory 变体仅有部分元数据和解压结果，不在逐函数等价结论内。

## 2. 授权层与成功语义

| 层次 | 输入及作用 | 已知成功条件 | 不能据此推导的结果 |
|---|---|---|---|
| 本地模式门槛 | UI、配置、内网探测 | 进入对应客户端分支 | 服务端授权通过 |
| 客服登录 | 账号、密码摘要、主机信息 | HTTP/JSON 检查、`ret == 0`、secret 匹配 | MES 或设备授权通过 |
| MES 登录 | 服务器、站点、两个登录输入 | `Login_MES` 返回非零，设置登录标志 | 所有生产信息已就绪 |
| PCBA 检查 | 登录状态、上传选项、站点及 PBL 信息 | 处理器返回 0，可能来自跳过分支 | 服务端实际批准该设备 |
| 本地许可证 | 主机绑定的签名、加密文件 | 验签、时间、字段及状态标记等门槛 | 原生工作函数返回 0 就已完整授权 |
| programmer token | 项目、PBL SN、时间等 | 客户端处理设备响应；规则因命令而异 | 客服、MES 或许可证状态有效 |

这些机制具有不同的字段、密钥和成功约定。特别是设备 XML 中的 `pk` 是 16 字节字符序列，参与 AES 密钥拼接；它不是许可证的 RSA 公钥。

## 3. 设备 token：5.1.75.0 与 5.1.77.0

本节采用 [多版本报告 §12](../msmdownloadtool-analysis.md#multi) 的最新差分证据。它更新了早期报告中“算法候选”“VM 提前退出”等阶段性结论，并修正短 SN 补零及 A/B/D 的符号解释。5.0.15.0 只确认了调用边界，不能套用本节的算法等价结论。

### 3.1 传输边界与输入

客户端向 programmer 发送以下 XML 命令。此处只展示命令元素，省略 Firehose 外层；占位符不是可发送数据。

```xml
<demacia token="{512 个大写十六进制字符}" pk="{16 个 ASCII 字符}" />
<setprojmodel token="{512 个大写十六进制字符}" pk="{16 个 ASCII 字符}" />
<setswprojmodel token="{1024 个大写十六进制字符}" pk="{16 个 ASCII 字符}" />
```

令 `S` 为下载对象，`C` 为配置对象，`T` 为 token 上下文。以下为内存布局，不是线上的二进制结构。

| 值 | T 偏移 / 类型 | 数据来源或含义 |
|---|---|---|
| P | `+0x08`，字符串 | 当前模式选择的项目 |
| M | `+0x20`，字符串 | 配置模型 |
| CUST | `+0x38`，字符串 | 配置客户字段 |
| TS | `+0x50`，字符串 | 第一次取时换算的 Unix 秒十进制文本 |
| H | `+0x68`，字符串 | 项目路径的 SHA-256 大写十六进制摘要 |
| W | `+0x80`，字符串 | 配置软件字段 |
| CF | `+0x98`，uint32 | 下载对象中的 CF 值 |
| SN | `+0x9c`，uint32 | PBL 查询结果的首 DWORD |
| A / B | `+0xa4 / +0xa8`，int32 | 软件项目路径的两个字段 |
| D | `+0xac`，uint32 | 软件项目路径的第三个字段 |
| DT | `+0xb0 / +0xb4`，uint64 | 软件项目路径使用的设备返回值，低 DWORD 在前 |

SN 不等于 Windows 卷序列号。新版来源为 `0x4c0bf0 → 0x4d2a40 → 0x4d2950(command=1)`；返回长度大于 0 且小于 `0x100` 时取首 DWORD。MES 把此数格式化为 `%x`，设备 token 则按下文的十进制规则处理。

| 对象或字段 | 5.1.75.0 | 5.1.77.0 |
|---|---|---|
| C / T 指针 | `S+0x90 / S+0x5a0` | `S+0x94 / S+0x5a4` |
| 项目选择模式 | `C+0xcc` | `C+0xe4` |
| 模式 1 / 2 的 P | `C+0 / C+0x18` | `C+0 / C+0x30` |
| M / CUST / W | `C+0xe4 / C+0xfc / C+0x30` | `C+0xfc / C+0x114 / C+0x48` |
| CF / SN | `S+0x58 / S+0x570` | `S+0x5c / S+0x574` |

其他模式不在此复制步骤覆盖 P；两版的模式 3 测试保留了预置值。新版软件项目准备函数 `0x4c1ef0` 由 `C+0xe8` 分派，值为 1 时先发送 `setprocstart`，其 64 位回复写入 DT。A 来自 `C+0x158`；B 由 `S+0x20 == 2` 置 1，否则为 0；D 按模式来自预读值 `S+0x5e8`、param 项 `0x27` 或 0。尚未确认的产品含义保留为 A/B/D。

证据：[输入复制测试](../msmdownloadtool-multi-version-evidence/token-5177/input-mapping-vectors.json)、[新版软件项目准备](../msmdownloadtool-multi-version-evidence/token-5177/bn/asm/0x4c1ef0.txt)。

### 3.2 时间与 pk

项目消息中的 TS 与生成 pk 的种子分别取时，可能跨秒。第二次时间值的低 32 位作为 PRNG 种子，执行 16 次以下递推：

```text
state = seed & 0xffffffff
alphabet = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"
每次：
  state = (state * 0x343fd + 0x269ec3) & 0xffffffff
  r = (state >> 16) & 0x7fff
  输出 alphabet[r % 62]
pk = 16 个输出字符拼接
```

种子 `1700000000` 对应 `4ZXOl2BrpW15edbO`。分离取时的测试证明 TS 可保留前一秒，而 pk 使用后一秒的种子。pk 分别存于 T 的 `+0x174`（setprojmodel）、`+0x184`（demacia）、`+0x194`（setswprojmodel）。

### 3.3 序列化规则

下列 `||` 表示无分隔字节拼接；`u32` 为无符号十进制，`s32` 为有符号十进制，`UHex` 为大写十六进制，`cstr` 截到第一个 NUL。公式中的固定字符串是 ASCII；输入字段表示已进入 T 的字节串，上游非 ASCII 转换尚未验证。

摘要按字符串对象的实际长度读取，包括内嵌 NUL。逗号消息中的字符串项按 `cstr` 读取，没有观察到 CSV 引号或转义。

```text
setprojmodel:
  H = UHex(SHA256("c4b95538c57df231" || P || u32(CF) || u32(SN) ||
                 W || TS || CUST || "5b0217457e49381b"))
  message = join(",", cstr(P), cstr(M), cstr(CUST), cstr(W),
                       u32(CF), u32(SN), TS, H)
  plaintext = message 后补零字节至 256 字节

demacia:
  SN10 = u32(SN) 左补 ASCII '0' 至 10 位
  Hraw = SHA256("2e7006834dafe8ad" || SN10 || "a6674c6b039707ff")
  plaintext = "907heavyworkload" || Hraw，后补零字节至 256 字节

setswprojmodel:
  H = UHex(SHA256("7016147d58e8c038" || P || u32(SN) || W || TS ||
                 CUST || "8f7359c8a2951e8c"))
  message = join(",", cstr(P), cstr(M), cstr(CUST), s32(A), s32(B),
                       cstr(W), u32(SN), u32(D), TS, H)
  plaintext = message 后补零字节至 512 字节
```

M 不参与上述项目摘要；CF 不参与软件项目摘要或已测序列化。demacia 的摘要是 32 字节原始值，不先转成 64 字符十六进制。

已验证的边界包括：SN=12 得到 `0000000012`；A=`0xffffffff` 得到 `-1`；B=`0x80000000` 得到 `-2147483648`；D=`0xfffffffe` 得到 `4294967294`。W=`A\0B` 的三个字节全部进入软件项目摘要，但消息中的 W 只有 `A`。以上差异不能通过统一的字符串序列化函数抹平。

### 3.4 密码变换

三条路径均为 `token = UHex(AES-256-CBC(key, IV, plaintext))`。上节已经把明文补齐为 16 字节的整数倍，不再使用 PKCS#7。

| 路径 | 32 字节密钥的拼接方式 | 16 字节 IV |
|---|---|---|
| setprojmodel | `hex(10456387e37e2371)` + `ASCII(pk)` + `hex(a2d4a0740fd32896)` | `9d614a1eac81c9b2d376d74931036379` |
| demacia | `hex(0163a0d1fde26711)` + `ASCII(pk)` + `hex(4827c208fbb0e6f0)` | `96e0790cae2bb4af684c36cb0bec49ce` |
| setswprojmodel | `hex(46a59730bb0d41e8)` + `ASCII(pk)` + `LE64(DT)` | `dc910d88e3c6ee65f0c744b40230ce40` |

表中的 `hex(...)` 表示解码成原始字节，不能直接拼接十六进制文本。DT=`0x5566778811223344` 时密钥末 8 字节为 `44 33 22 11 88 77 66 55`。两条 256 字节明文生成 512 字符 token，512 字节明文生成 1024 字符 token。

### 3.5 响应与执行证据

5.1.75.0 的 demacia/setprojmodel 客户端主判断是 `response` 的 `value == "ACK"`。非 ACK 路径再解析 `verify_res`，或 `auth_token_verify`、`model_check` 等字段；不能把“所有字段无条件同时等于 0”写成统一成功规则。软件项目路径还涉及 `model_check`、`device_proj_name` 和模型不匹配分支。

| 响应诊断字段 | 已识别值的客户端解释 |
|---|---|
| demacia 校验结果 | 1：hash mismatch；2：magic/decrypt failed |
| 项目 token 校验结果 | 1：verify failed；2：model hash failed；3：SN failed |
| token 结果为 0，但 model_check 非 0 | 镜像/型号不匹配路径 |

这些是客户端响应处理证据，未以真实 programmer 验证返回条件。新版软件项目传输后的部分受保护调用仍不能仅按 HLIL 结构下结论。参见 [5.1.75 报告 §7.1](../msmdownloadtool-analysis.md#v5175)。

| 原始代码执行链 | 5.1.75.0 | 5.1.77.0 | 成对案例 |
|---|---|---|---:|
| setprojmodel | `0x4d4e00 → 0x4d4e80 → 0x4d44d0` | `0x4d5070 → 0x4d50f0 → 0x4d4740` | 14 |
| demacia | `0x4d4830` | `0x4d4aa0` | 10 |
| setswprojmodel | `0x4d4e00 → 0x4d5340 → 0x4d4680` | `0x4d5070 → 0x4d55b0 → 0x4d48f0` | 15 |

39 对、78 次目标执行逐项比较实际 SHA 输入、摘要、16 次 rand 返回、pk、key、IV、完整明文及完整 token，两版全部相同。另以 hashlib、OpenSSL AES 和整数递推独立复算。39 对已经包含 3 对堆初值控制；六组输入复制和六组 VM 运行时控制单独计数。

证据：[主差分向量](../msmdownloadtool-multi-version-evidence/token-5177/token-differential.json)、[边界差分](../msmdownloadtool-multi-version-evidence/token-5177/boundary-differential.json)、[复核结果](../msmdownloadtool-multi-version-evidence/token-5177/verification.json)、[证据哈希](../msmdownloadtool-multi-version-evidence/token-5177/SHA256SUMS.json)。

等价结论限于已测逻辑输入和已恢复运行依赖。未完整执行 Windows 启动及 T 构造器；未得到通用 VM 的全路径 IR。消息最长测试为项目 202 字节、软件项目 198 字节，未覆盖达到或超过 256/512 字节的全部行为，也未验证真实设备接受。

## 4. 客服 HTTP 登录

### 4.1 请求

5.1.75.0 和 5.1.77.0 的客户端包含 `POST https://csdt.1plus.io/downloadTool/login`，使用 `application/json` 及 `User-Agent: DLTool/V%d.%d.%d`。这是样本常量，不表示该服务现在仍可用。JSON 的六个值均为字符串：

| 键 | 内容 | 覆盖边界 |
|---|---|---|
| `account` | 账号输入 | 原生构造链已确认 |
| `password` | 下述 MD5 小写十六进制文本 | 非 ASCII 转换/长度未验证 |
| `ma` | 适配器采集结果 | 未完整恢复所有规范化规则 |
| `sn` | 客服主机信息采集结果 | 精确采集来源未闭合，不能直接套用许可证的 C 盘序列号 |
| `hn` | 主机名 | 5.1.75 已解析到物理 DNS 主机名的 `GetComputerNameExW` |
| `ts` | `%llu` 格式的十进制时间值 | 不是随机数 |

```text
joined = "9r4loFcc2Cs6" + supplied_password + "cX6UiiQ2aOrM"
bytes = wide_to_multibyte(joined, conversion_argument=3)
password = lowercase_hex(MD5(bytes, length_from_joined_CString))
```

固定前后缀不是每次登录生成的盐。转换实参 3 不能直接写成 UTF-8；摘要长度来自原 CString，非 ASCII 时必须另行核验。原生 MD5 的局部向量验证不能替代完整登录验证。

证据：[5.1.75 请求构建](../msmdownloadtool-5.1.75-evidence/main/hlil/0x4575d0.txt)、[5.1.75 密码预处理](../msmdownloadtool-5.1.75-evidence/main/hlil/0x457400.txt)、[5.1.77 密码预处理](../msmdownloadtool-multi-version-evidence/op9pro/hlil/0x457560.txt)。

### 4.2 响应和状态

5.1.75.0 的解析顺序为 HTTP 200 → JSON 字段检查 → secret 比较 → 业务状态处理。必须存在字符串 `secret` 和整数 `ret`；`ret == 0` 时要求字符串 `scCode`、`region`，否则要求整数 `error`。正常 ASCII 输入下：

```text
K = convert(ma + hn + sn)
M = convert(ma + hn + sn + account + ts)
expected_secret = UpperHex(HMAC-SHA256(K, M))
```

转换同样使用实参 3 和 CString 长度。password 不在该 HMAC 输入中。这是客户端可计算的一致性检查，现有证据不支持将其称为服务端私钥签名。

| 条件 | 5.1.75.0 本地状态 |
|---|---:|
| HTTP 非 200 | 1 |
| 必需字段缺失或解析失败 | 11 |
| secret 不匹配 | 9 |
| 服务 error 为 0 / 1 / 2 / 3 / 4 | 4 / 5 / 7 / 6 / 8 |
| 其他服务 error | 12 |

缓存账号、处理后的密码、机器信息和时间值不单独代表业务成功；需结合 `ret == 0` 等条件。5.1.77.0 已确认对应客服构造和摘要结构，但没有真实 HTTP 往返，不能将全部 UI 状态映射自动推广到该版。原始报告另有忘记密码常量及请求/响应日志回调，未恢复成完整协议。

证据：[5.1.75 报告 §5](../msmdownloadtool-analysis.md#v5175)、[多版本报告 §7.1](../msmdownloadtool-analysis.md#multi)。

## 5. MES 登录与 PCBA 状态机

### 5.1 DLL 边界

5.1.75.0 与 5.1.77.0 包中的 `libMESAPI.dll` 字节相同，SHA-256 为 `1cc1b9f381065074defd6dc56d6a4dbf87e6c1971f32f84a420ffc0fbab6161c`。可复用 DLL 分析，不代表两个主 EXE 的全部逻辑相同。

客户端先选择服务器、工厂/站点和 service 路径，调用 `SetServiceConfig(0, path)`，再把四个 CString 交给 `Login_MES`：服务器、工厂/站点、两个 UI 登录输入；另传消息输出引用。

| Login_MES 返回 | 客户端动作 |
|---|---|
| 0 | 清除登录标志，显示失败；没有证明同时清空全部旧缓存 |
| 非 0 | 缓存四个输入，置登录标志，保存站点配置并调用 GetStandardInfo |

非零成功不意味着 DLL 只返回 0/1，也不意味着生产信息加载、批次选择等后续步骤全部完成。

### 5.2 PCBA 门槛与参数

下载主链通过消息 `0x1b74` 进入 PCBA 处理器：5.1.75.0 为 `0x4415c0`，5.1.77.0 为 `0x441790`。判断顺序如下：

| 顺序 | 条件 | 客户端结果 |
|---:|---|---|
| 1 | MES 登录标志为 0 | 返回 1，失败 |
| 2 | 已登录，MES Upload 关闭 | 记录跳过，返回 0 |
| 3 | 已登录且开启上传，SMT 标志等于 1 | 记录跳过，返回 0 |
| 4 | 其余情况，参数分配失败 | 返回 1 |
| 5 | 参数就绪，VerifyPcbaUpgrade 返回非零 | 返回 0 |
| 6 | VerifyPcbaUpgrade 返回 0 | 返回 1 |

关闭上传与 SMT 同时成立时先走关闭上传分支。“返回 0”可能仅表示客户端跳过请求，不能记为服务器批准。

5.1.75.0 构造的六个 CString 依次是：缓存服务器；PBL 首 DWORD 的 `%x` 文本（无固定补零）；按配置选择的软件信息字段 A；空串；空串；软件信息字段 B。A/B 的完整业务命名尚未确认。这些 MES 字段 A/B 与设备 token 上下文的数值 A/B 不是同一组符号。

原始机器码的条件组合测试在失败、跳过或调用准备入口停止，没有执行真实 MES 请求。该处理器的失败分支未见清除登录标志或重试调用。

证据：[5.1.75 报告 §12.3](../msmdownloadtool-analysis.md#v5175)、[多版本报告 §7.2](../msmdownloadtool-analysis.md#multi)、[新版 PCBA 证据](../msmdownloadtool-multi-version-evidence/op9pro/asm/0x441790.txt)、[门槛向量](../msmdownloadtool-multi-version-evidence/pcba-gate-vectors.json)。

### 5.3 尚不能写成线协议的部分

DLL 中已有 HTTP、Basic、RSA、`random_number`、`s_msg_md5` 及 `AateAsyApiCheckAuthenForUpgrade` / `AateAsyApiPcbaUpgrade` 的字符串或初始化证据，但内部调用链、请求字段、编码、签名拼接和服务端状态机没有完整恢复。因此本节给出的是 DLL ABI 与客户端状态机，不提供猜测的 MES HTTP 请求模板。

## 6. 本地许可证：5.1.75.0 原生核心

本节来自 [5.1.75 报告 §11.4–11.5](../msmdownloadtool-analysis.md#v5175)。5.0.15.0 和 5.1.77.0 存在许可证 UI/调用边界，尚未证明其文件算法与本节完全相同。受保护包装器 `0x440140` 到工作函数 `0x4ad930` 的具体调用边仍未闭合。

### 6.1 许可证申请文件

申请 token 的明文为 `hn:ma:sn:ts`，各字段在此路径的来源为：

| 字段 | 来源 |
|---|---|
| hn | GetComputerNameExW |
| ma | 枚举适配器，取 IP 文本不为 `0.0.0.0` 的项，物理地址逐字节大写十六进制 |
| sn | 本机 `C:\` 卷序列号，以两组十六进制和连字符表示 |
| ts | Unix 秒十进制文本 |

宽字符转换实参为 3；转换后字节串交给 RSA-2048 公钥加密，填充为 PKCS#1 v1.5。密文以大写十六进制显示，正常长度 512 字符。公钥指数为 65537，SPKI DER SHA-256 为 `a53e08f79748be773adccfb9130448816a71952dedd05378b808943774b30c67`。

默认申请文件为 `License4Token.enc`。可见保存路径以 `CREATE_ALWAYS` 写入宽字符串长度乘 2 个字节，即 UTF-16LE 文本；未见 BOM 或终止 NUL。打开/写入失败可能只记日志而最终仍返回 0，因此 UI 保存成功不能独立证明文件写入。RSA 填充包含随机性，不能要求同一申请输入的密文恒等。完整 RNG 路径及签发过程未执行。

### 6.2 License4.dat 容器

前 12 字节是六个大端 uint16；其后按如下位置读取：

| 文件偏移 | 长度 | 含义 |
|---|---:|---|
| `0x00` | 2 | magic，要求 `0x0332` |
| `0x02` | 2 | 所见主链未赋予语义 |
| `0x04` | 2 | 声明总长度 L；要求实际文件长度至少为 L |
| `0x06` | 2 | 所见主链未赋予语义 |
| `0x08` | 2 | 密文长度 C |
| `0x0a` | 2 | 签名长度 S |
| `0x0c` | 16 | IV |
| `0x1c` | C | 加密负载 |
| `L-S` | S | 签名 |

同一 RSA 公钥配合 SHA-256 验证 `file[0x0c : 0x0c+16+C]`，即 IV 和密文，不包含 12 字节头。验签过程失败为 `0x202a`，签名不匹配为 `0x241`。这不是畸形长度或所有重叠布局均已审计的声明。

正常 ASCII 输入的 AES 派生为：

```text
aes_key = MD5_raw16(hn + ":" + ma + ":" + sn + ":6bb91c30390d8718")
payload = AES-128-CBC-decrypt(aes_key, file.IV, file.ciphertext)
```

解密后截取 JSON 边界并解析 `User`、`VerifyCode`、`GenTime`、`ExpTime`、`ServerID`。现有证据未给出可独立验算的完整签发文件、全部填充处理及非 ASCII 规则。

### 6.3 时间、绑定摘要和最终状态

时间要求为 `GenTime ≤ now ≤ ExpTime`。另有 HKCU 历史时间 DWORD 与当前时间低 32 位的比较及初始化；这属于本地回拨检查，不是服务端时间证明。

对已分析的 ASCII 路径，HMAC 拼接如下，GenTime/ExpTime 使用已装入对象的文本表示：

```text
P = User + hn + ma + sn + ServerID
message = "3d76ca2c926f758d" + P
key = P + GenTime + ExpTime
expected = UpperHex(HMAC-SHA256(key, message))
```

校验函数先清空状态标记，仅在 expected 与 VerifyCode 相等时写入 `cbbdcaaca74088039b03d81ad7766ef3`。工作函数未传递字段装载及 HMAC 校验的全部返回值，局部返回 0 不能代替标记、全局状态及上层门槛。时间检查返回 5 映射为 `0x1b90`，其他时间失败映射为 `0x576`；UI 文案不能当作服务端判断。

证据：[工作函数](../msmdownloadtool-5.1.75-evidence/main/asm/0x4ad930.txt)、[验签](../msmdownloadtool-5.1.75-evidence/main/asm/0x4af550.txt)、[AES 包装](../msmdownloadtool-5.1.75-evidence/main/asm/0x4b08a0.txt)、[HMAC 与标记](../msmdownloadtool-5.1.75-evidence/main/asm/0x4ae8e0.txt)。

## 7. 旧版本门槛

3.0.2.0 的本地模式/密码函数为 `0x40a5b0`，SMT 等分支检查主窗口状态。已审计链未恢复出在线账号协议；这不足以证明整个程序没有任何网络行为。

4.0.28.0 对 `pjira.oneplus.cn`、`romsign.onepluscorp.cn`、`mo.adc.com` 等名字解析并用 raw ICMP 探测。可见逻辑发送含 HelloWorld、计数和时间值的数据，select 等待约 5 秒；出现可读状态即可判定探测成功，未见读取并核对来源、序号和 payload 的步骤。结果主要影响无有效 OPS 时的明文 Setting.xml 分支，不能当作远端身份认证。

5.0.15.0 刷写链在配置条件满足时先 demacia，再 setprojmodel；内部受保护 token 生成入口已定位，但没有本节三类 token 的输出等价验证。

证据：[多版本报告 §5.1、§6](../msmdownloadtool-analysis.md#multi)、[4.0.28 报告 §5](../msmdownloadtool-analysis.md#v4028)。

## 8. 尚未闭合的协议边界

| 项目 | 缺少的证据 |
|---|---|
| 客服协议 | 非 ASCII 转换、完整主机采集规范、真实请求/响应和服务端授权策略 |
| MES | DLL 内部线报文、加密/签名组合、服务端状态及 PCBA 实际响应 |
| 许可证 | 包装器调用边、完整已签发文件、异常长度/填充/RNG 和跨版本一致性 |
| 设备 token | 超长/字符集边界、完整启动环境、所有模式、设备接受及校验实现 |
| 版本覆盖 | 5.0.15.0 密码核心、Win7/factory 变体及未分析 programmer 的等价性 |

上述缺口不影响已记录向量的可复核性，但限制了把客户端提取结果当作完整服务或设备协议规范的范围。

## 9. OplusFlashTool 5.9.18.0：RCSM 与设备签名

本节单独适用于官方 Sales 包的 `DownloadTool.exe`，原始 SHA-256 为 `8f83cbf571adcf76e2d8e960e36a05486bf5eef50c46877335710dfece86a711`。DLL 地址均为模块内首选 VA，ImageBase `0x10000000`。样本、各 DLL 摘要及详细证据见 [5.9.18 报告](../oplusflashtool-5.9.18-analysis.md)。**第 3 节旧 MSM 设备 token、客服密码摘要及第 6 节 License4.dat 算法没有被证明适用于此版。**

### 9.1 RCSM 登录

GUI `0x420940 → Bridge!0x1000fcc0 → Base!0x1001bf00 → Base!0x10018cb0` 构成 type 4 登录链。服务器配置列出中国、印度、欧洲和其他地区 RCSM；Sales 可显示验证码入口。请求路径 `/api/tools/login` 的参数 map 包含：

```text
board_id, cpu_id, disk_id, ip, mac,
user_id, password, verification_code,
login_type="1", version="0"
```

`password` 是本层输入字段，未证明其线上字节等于 UI 明文。低层状态依次从 `/Data/response`、`/Data/response/message`、`/Data/message` 获取，要求 `0000`，并要求存在 `/Data/response/countryname` 与 `/Data/response/token`。token 保存到 `Base!0x1021b480`。`usertype` 缺失仅在此低层记录错误，并不阻止返回 0。真实登录、验证码和 token 期限未测试。

外层为 `application/x-www-form-urlencoded`，字段为 `app_id, timestamp, sign, s_msg, s_msg_md_5`，条件追加 `s_msg_c`。`Base!0x1002b140` 按 path、app_id、s_msg、可选 s_msg_c、s_msg_md_5、timestamp、helper+0x244 的顺序用换行标签拼接并调用 `op_md5_str`。完整公式、helper 偏移和依据见 [报告 §4.2](../oplusflashtool-5.9.18-analysis.md#42-rcsm-外层封装与-vm-缺口)。s_msg 生成入口 `0x103a6d6e` 和响应入口 `0x104e9413` 仍受保护，不能把已知外层写成完整可用的登录协议。

### 9.2 设备挑战、服务器签名与本地预校验

`OplusQCom!0x1001cc50` 先发送 `getsigndata`，再由 `0x100210e0` 解析分号字段。令 R = worker+0x3c：

| 输入 | R 偏移 | `/api/tools/sign` 字段 |
|---|---:|---|
| Version | 0x000 | meta_ver、version |
| Platform1 / Platform2 | 0x100 / 0x200 | main_platform / sub_platform |
| Rand | 0x400 | random_num |
| ProjectWrite / ProjectRead | 0x500 / 0x600 | new_project_no / old_project_no |
| DigestWrite / DigestRead | 0x700 / 0x800 | new_sw_name_sign / old_sw_name_sign |
| ID | 0x900 | chip_sn |

RCSM sign 另带主机信息、登录 token，以及固定 `read_write_mode=W, lock_ver=1, login_type=1`。R+0x300 的设备 Mode 单独用于本地 OSIG；不能与固定 read_write_mode 合并。响应 encrypt 经 Base64 解码，随后还要通过本地预校验及设备 `verify`。

`Base!0x1001d420` 生成 **208 字节 OSIG**：固定头、版本/扩展字节、Y/N 标志、Mode、Platform1/2、ID、Rand、DigestWrite/Read、ProjectWrite/Read，再计算 SHA-256。精确布局见 [报告 §5.1](../oplusflashtool-5.9.18-analysis.md#51-208-字节结构)。4 组合成输入直接执行原生序列化及 SHA，全部与参考一致；未执行 RSA 验签、N 重试或真实挑战。

客户端先尝试扩展 OSIG，再尝试非扩展 OSIG；只在特定签名成功但预校验失败路径重取签名，最多 5 次。服务错误通常立即返回。不能把这些次数理解成网络层统一重试。

### 9.3 成功语义与剩余缺口

签名响应的状态比较与登录不同。`Base!0x100197bd` 原生比较测试中，空串进入 encrypt 查找，`0/0000/1001` 均拒绝。受保护响应处理仍未知，不应擅自将其修写成“0 或 0000 成功”，也不能据此判定真实服务故障。

设备 `verify` 发送 XML、等待 ACK、发送原始签名缓冲区，并要求返回包含 `verfiy passed` 或 `verify passed`。SFP 中长度参数为 4096，本地 RSA 预校验检查的签名长度为 256；两个边界不等价。`new_package_flag` 的特定兼容分支仍通向设备 verify，不证明免授权。

待确认：UI 密码预处理、s_msg 序列化/密码变换、响应展开、真实 token 生命周期、工单权限、平台公钥/RSA 完整实现、RMX3800 programmer 的接受规则。本轮没有联网认证或设备写入。

证据：[登录](../oplusflashtool-5.9.18-evidence/base/hlil/0x10018cb0.txt)、[签名参数](../oplusflashtool-5.9.18-evidence/base/hlil/0x10019410.txt)、[OSIG 原生验证](../oplusflashtool-5.9.18-evidence/osig-native-validation.json)、[状态比较验证](../oplusflashtool-5.9.18-evidence/rcsm-sign-guard-validation.json)、[设备 verify](../oplusflashtool-5.9.18-evidence/oplusqcom/asm/0x10006c60.txt)。

## 10. Anteasy OplusTool 4.2026.009：一加离线 token 与 SWM

这是第三方工具，不能与上一节的官方 OplusFlashTool 混为一谈。样本、SHA-256 和完整证据见 [专项报告](../anteasy-oplustool-4.2026.009-analysis.md)。

`Oplus_Qualcomm.dll` 的 `0x180002a10 / 0x180002cd0` 本地生成 demacia / setprojmodel。输出均为 512 个大写十六进制字符；密钥拼接、IV、零填充和字段顺序与第 3 节对应协议一致。它固定 `M=TdYWNqblwrWOJ1Yy`、`W=Oplus_downloadtool`，并令 `CUST=UHex(SHA256("7016147d58e8c038" || P || M))`。这不是任意固件配置的完整复刻，接受范围仍由实际 programmer 决定。

EXE 的 `0x1405a0d40 → 0x1405a6ea0` 在有效项目路径中先申请 SWM 操作枚举 5，SWM 将其映射为 `qualcomm.oneplus_offline_auth`。授权完成后才调用设备认证循环；服务缺失返回错误。SWM 使用 `/api/client/operation-authorizations` 和 `/consume`，其 operation grant 与官方 RCSM token、设备 pk 分属不同协议。

本轮 8/8 局部执行案例验证了原始机器码的序列化、密码输入和输出编码。SHA/AES 包装使用标准实现适配器，因此不能写成完整原生密码差分或设备验签成功。证据：[局部验证](../anteasy-oplustool-4.2026.009-evidence/token-serialization-validation.json)、[EXE 前置授权](../anteasy-oplustool-4.2026.009-evidence/app/hlil/0x1405a0d40.txt)、[SWM 分派](../anteasy-oplustool-4.2026.009-evidence/swm/hlil/0x180046e80.txt)。

该路径没有证明能替代 RMX3800 的 `getsigndata → RCSM sign → verify`。发布说明仅声明离线授权支持部分一加 7/8/9；主程序存在 RMX3800 目录项也不改变这一证据边界。

## 11. 云端 loader 授权与 RMX3800 设备端 OSIG

追加分析见 [Anteasy 报告第 9 节](../anteasy-oplustool-4.2026.009-analysis.md#9-云端-loader-与-rmx3800-对照样本的继续追踪)。云端获取链为 `SWM enrollment ticket → loader enroll → 临时 client_id/client_secret/delegation_id → 签名 list/ticket → 资产校验`。enrollment ticket、operation grant、下载票据和设备 OSIG 签名分别有不同消费者，不能互相替代。一次匿名列表请求实测返回 HTTP 401 `signature_required`，未取得 RMX3800 的云端资产或其真实 profile/asset ID。

对照样本是 realme 官方 RMX3800 16.0.5.740 服务包内的 programmer，SHA-256 为 `daaddec2d87fa2f30a527f0e3d656044ef9e2fdece8163afde856cb9505e34ea`。Binary Ninja 已确认嵌套 AArch64 模块的 `getsigndata → verify → OSIG/RSA → 授权状态`。verify 接收 4096 字节，签名检查使用前 256 字节；OSIG 为 208 字节。Version=0、QCOM/SM8650、W/R、Rand、摘要和 ID 与主机端布局对应，该样本的 ProjectWrite/Read 是 `00000`，不能替换为目录中的 23631。

3 组合成输入下，loader 原始序列化与官方 5.9.18.0 非扩展模式、首次 Y 的主机端原生输出逐字节相等。首轮 loader SHA 失败已在[后续分析第 10 节](../anteasy-oplustool-4.2026.009-analysis.md#10-官方-rmx3800-loader-的-sha-环境修复与-rsayn-执行验证)定位为离线堆未初始化：`0x78` 字节上下文分配返回空指针，上层没有传播错误。输出预填 A5 时保持 A5，不能把先前缓冲全零视为计算结果。

调用原始堆初始化并提供仅供堆校验字段使用的 4/2/2 字节环境输入后，原始 SHA 12/12 通过，其中上述 3 组 OSIG 的摘要与已保存主机输出相等。原始验签协议为 RSA-2048、指数 65537、PKCS#1 v1.5/SHA-256，比较完整 `00 01 FF...00 || DigestInfo` 编码块；测试密钥用例 6/6 符合预期。

固定厂商公钥下，三组全零签名和一组测试密钥签名均实际走完 Y→N，两次失败后返回错误，初始为零的授权类型和总通过位仍为零；长度参数 255 在入口拒绝。这 5 例没有修改公钥或验签结果。Y 成功写类型 1、N 成功写类型 2、外层接收成功后设置总通过位仍属静态确认，尚无厂商有效签名驱动的动态成功证据。局部堆熵适配不证明设备 Rand 的真实生成，也不证明某个在线服务是唯一签发方。

证据：[云端匿名访问](../anteasy-oplustool-4.2026.009-evidence/cloud-loader/anonymous-list-attempt.json)、[loader 验签路径](../anteasy-oplustool-4.2026.009-evidence/cloud-loader/rmx3800/asm/0x149a1224.txt)、[历史输出差分](../anteasy-oplustool-4.2026.009-evidence/cloud-loader/osig-loader-native-validation.json)、[SHA 环境验证](../anteasy-oplustool-4.2026.009-evidence/rmx3800-crypto/sha-environment-validation.json)、[RSA/Y/N 验证](../anteasy-oplustool-4.2026.009-evidence/rmx3800-crypto/rsa-authorization-validation.json)。

### 11.1 官方 RMX3800 getsigndata 的字段生产

相同样本的[字段数据流追加分析](../anteasy-oplustool-4.2026.009-analysis.md#11-getsigndata-字段来源存储记录与离线执行)已恢复以下关系：

| 字段 | 原始数据流与编码 |
|---|---|
| ID | 读取 `0x221c8610` 的 32 位值，拒绝 0/FFFFFFFF，以 `%x` 输出小写十六进制，不补前导零；OSIG 消费 8 字节槽位 |
| Rand | 本次已验证硬件路径每两个来源字异或，生成 16 字节；上层只取前 8 字节作为小端 uint64，再输出 16 位小写十六进制 |
| DigestWrite | 由前序接收并通过检查的签名摘要表缓冲计算 SHA-256，小写 hex64；getsigndata 只读缓存。本轮以官方 33144 字节表验证摘要函数，未验证上游安全世界签名表接受 |
| DigestRead | UFS unit 3/LBA 0x260 或 eMMC unit 2/LBA 0x26 读取旧记录；固定公钥验签成功后取记录 `+0x40` 的 64 字节。失败使用内置默认串，成功复制分支尚缺有效厂商签名的动态正例 |
| NV | UFS unit 5 或 eMMC unit 0 定位 oplusreserve1 中的记录；检查 OSIG 魔数，原始 Base64 编码前 352 字节得到 472 字符。独立输出，不加入 208 字节当前挑战 OSIG |

旧记录验签重建 OSIG 时使用当前芯片 ID、记录中的 Rand/两摘要及固定 W 模式，签名为记录 `+0x100` 的 256 字节；它不使用当前新生成的 Rand 或 DigestWrite 缓存。安全状态查询失败仍走安全路径。普通读取/格式失败可以使 NV 为空或 DigestRead 为默认值，而挑战准备成功；准备位不等于设备授权通过。

离线完整入口 10/10 用例符合预期，止于 ACK 发送前。寄存器、安全监控与块设备输入均是明确的模拟环境，RSA、SHA、GPT 解析和 Base64 保持原始代码；没有真实手机字段或在线服务接受证据。GPT 查找未命中时还存在未初始化栈输出的依赖，不能把所有失败都解释为固定默认偏移。证据：[完整执行](../anteasy-oplustool-4.2026.009-evidence/rmx3800-getsigndata/getsigndata-integrated-validation.json)、[存储记录绑定](../anteasy-oplustool-4.2026.009-evidence/rmx3800-getsigndata/record-dataflow-validation.json)、[偏移选择验证](../anteasy-oplustool-4.2026.009-evidence/rmx3800-getsigndata/storage-selector-validation.json)。

### 11.2 官方 RMX3800 的表认证与持久签名记录

此补充只适用于上述固定 SHA-256 的官方 loader。完整地址、样本与验证记录见[安全世界边界追加报告](../anteasy-oplustool-4.2026.009-analysis.md#12-签名摘要表的安全世界边界与真实旧记录搜索)。

表认证经 `0x149a4cf4 → 0x149c2988` 发出 `SMC 0x02000201`：X1=`0x14`，X2=表地址，X3=长度，X4=`0x1a`。地址须按 4096 字节对齐，调用前执行缓存维护。接受条件同时要求 SMC 返回 W0=0 和返回 X1 对应的 64 位服务状态为零，之后消费返回载荷地址/长度并检查 SVIP 结构。已原生验证调用边界及拒绝路径，未提供安全世界验签成功替身。

官方包六个签名表采用 ECDSA P-384/SHA-384；六次验签、六次单比特篡改拒绝和 12 项段哈希均符合预期。证书链签名以包内根、关闭时间检查的条件验证通过。它证明文件内密码一致性，不证明设备根和安全世界接受。DigestWrite 对整个签名表使用 SHA-256；OSIG 则使用 RSA-2048/PKCS#1 v1.5/SHA-256，三者不能互换。

DigestRead 的 UFS 位置经包内 GPT/XML 确认对应 dinfo（LUN 3、LBA `0x260`）。`writever` 的原生构造器清零 4096 字节，仅写 `+0x08` 的 Y/N、`+0x30` 的 Rand、`+0x40/+0x80` 的读写摘要和 `+0x100` 的 256 字节签名。该持久格式不含 OSIG 头和设备 ID；读取验签时才用当前 ID 重建 OSIG。NV 对 OSIG 魔数的要求属于另一记录格式。

两种授权类型的格式用例逐字节一致，但未设置授权通过位、未执行获准写入。68 个已下载成员中没有原生格式的真实记录；唯一宽松形状命中为 loader 的字符串/公钥常量。dinfo 的 XML 输入文件为空，不能推断设备数据为空。有效厂商 RSA 记录、对应芯片 ID、DigestRead 成功复制及真实安全世界策略仍待验证。

证据：[SMC/序列化验证](../anteasy-oplustool-4.2026.009-evidence/rmx3800-secure-auth/boundary-validation.json)、[官方表密码验证](../anteasy-oplustool-4.2026.009-evidence/rmx3800-secure-auth/signed-table-crypto-validation.json)、[记录搜索与 GPT](../anteasy-oplustool-4.2026.009-evidence/rmx3800-secure-auth/record-search-and-partitions.json)。
