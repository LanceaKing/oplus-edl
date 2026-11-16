# MSMDownloadTool 研究资料与 RMX3800 路径的迁移评估

结论：**研究方法、字段核验方法和通用协议验收案例可以迁移；现有 MSM 设备 token、许可证和 MES 流程不能直接用来补齐 RMX3800 的会话签名。** 当前仍应追踪与 `getsigndata → verify` 匹配的客户端及签名来源，不改为实现旧版 OnePlus token 后假定兼容。

本次仅阅读本地资料、解析已有公钥、核对证据哈希和扫描已有加载器字节。没有运行 MSM 程序、重新执行 token 机器码、联网、操作设备或改产品代码。研究原件保持原位；本页引用它们，不覆盖旧报告。

## 对照范围及证据强度

主要依据为 [auth.md](../../research/msmdownloadtool/auth.md)、[flash.md](../../research/msmdownloadtool/flash.md)、[ota.md](../../research/msmdownloadtool/ota.md)，并回查 5.1.77 的三个 XML 命令构造函数、5.1.75 的许可证验签函数、公钥 PEM 和 token 差分证据。旧报告覆盖 OnePlus 3／6／7T／8／9 Pro、主 EXE 3.0.2.0 至 5.1.77.0；当前对象是 RMX3800 16.0.5.740 包内 SM8650 加载器，不能因同属 Oplus 工具就共享协议或地址。

本轮重新核对 `token-5177/SHA256SUMS.json` 所列的 **165 个文件，全部匹配**。已有验证记录为 39 对、78 次目标代码执行，证明指定合成输入下两版客户端 token 数据流一致；本轮没有重做这些执行，也没有新增设备接受证据。相关报告快照哈希、逐把公钥比较和加载器字面扫描结果见 [msm-comparison.json](msm-comparison.json)。

## 认证路径是否相同

| 比较项 | MSM 研究已恢复内容 | RMX3800 当前加载器 | 迁移判断 |
| --- | --- | --- | --- |
| 设备入口 | demacia、setprojmodel、setswprojmodel；软件路径另有 setprocstart | getsigndata、verify；另有 SVIP 表与 sha256init 状态 | 两套命令链，没有已证实的转换关系 |
| 线格式 | XML token 属性为 512／1024 个十六进制字符，另带 16 字符 pk | verify 接收 4096 字节；前 256 字节签名用于验证 208 字节消息 | 解码后的 256 字节 token 与 256 字节 RSA 签名仅长度相同，不可互换 |
| 密码算法 | 项目／序列号／时间等组成消息，经 AES-256-CBC 得到 token | 固定 RSA-2048 公钥验证 SHA-256／PKCS#1 v1.5 签名 | 不能将 AES token 生成器移植为当前 sig 生成器 |
| pk／Rand | pk 是客户端取时后 PRNG 生成的 16 字符串，参与 AES key 拼接 | Rand 是加载器运行时数值转出的 16 个十六进制字符，进入待验消息 | 同为 16 字符不代表同一来源或字段 |
| 身份字段 | PBL 首 DWORD 作为 SN；各 token 路径用十进制，demacia 补至十位 | 从加载器芯片序列路径取值，ID 用 `%x` 文本，复制前 8 字节 | 业务上都与设备身份有关，编码及绑定方式不同，不移植补零规则 |
| 项目／版本 | 软件、客户、项目、模型等来自客户端配置 | OSIG 消息含固定 Version 0、平台字段、两处 00000，以及 ID／Rand／双摘要 | 不能将客户端项目号直接填入当前 ProjectWrite／Read |
| 签名或令牌来源 | 已恢复旧客户端本地 token 计算；许可证签发及 MES 内部协议仍有缺口 | 当前缺与该公钥及会话消息相配的合法签名来源 | 旧研究没有补齐这个核心缺项 |
| 成功依据 | 各命令按 ACK、诊断字段、客户端模式分别判断 | 前序状态、raw 接收、RSA 验签状态和后续权限须分别确认 | 可迁移分层判断方法，不能搬旧响应字段或宽松成功判断 |

本轮在当前 programmer 全文件中找到 getsigndata／OSIG 字面值，没有找到 demacia、setprojmodel、setswprojmodel、setprocstart 的 ASCII 或 UTF-16LE 字面值。对 MSM 已有报告及导出的 md/txt/json 检索，也未找到当前 getsigndata、DigestWrite／DigestRead、OSIG 或 AUTH_INTRANET 的对应记录。**这是已查材料范围内的负面证据，不证明所有间接分派、编码字符串或未分析版本均不存在这些功能。**

三条旧 XML 入口的原始导出依据：[demacia](../../research/msmdownloadtool-multi-version-evidence/op9pro/hlil/0x4ce6e0.txt)、[setprojmodel](../../research/msmdownloadtool-multi-version-evidence/op9pro/hlil/0x4cedb0.txt)、[setswprojmodel](../../research/msmdownloadtool-multi-version-evidence/op9pro/hlil/0x4cf540.txt)。当前消息与公钥依据见[会话协议分析](session-protocol.md)。

## 公钥确实不同，不能混用许可证或 MES 材料

为避免比较不同编码，本轮将两份 PEM 中的 RSA 模数解析成大端 256 字节，再与当前加载器模数逐值比较。三者都是 2048 位、指数 65537，但模数不同：

| 公钥用途 | 大端模数 SHA-256 |
| --- | --- |
| RMX3800 verify | `56cceb8ef82b2428d534769c09898c689d5cd718c42922a416c9c77c85800331` |
| MSM 5.1.75 许可证申请／验签 | `be82757954de5ce78de38ab7b375959b18094d2a95124cd82c0bed1e75b4533f` |
| MSM 已提取 MES 公钥 | `20d34b8c091761ea085f7dc32e38ae8e2850f4c6c8de498af999b041eeb1e6fb` |

旧资料记录的 SPKI DER 哈希与本表不是同一种摘要对象；本轮另核对了 DER 哈希与旧记录一致。许可证 `License4.dat` 验证的是 IV＋密文，申请 token 加密的是主机 hn/ma/sn/ts；两者均不是当前加载器的 OSIG 会话消息。MES 公钥只证明有该常量，旧研究自身也未恢复完整签名请求／响应链。客服 login 的 secret 是 HMAC 一致性检查，不是该 RSA 会话签名接口。

因此，不能把 License4.dat、License4Token.enc、旧 token 或客服登录成功状态作为当前缺失 Sig 的替代，也不能从旧端点名称推导出 RMX3800 签发 API。

## 可迁移的部分

| 内容 | 本轮迁移方式 | 后续实现边界 |
| --- | --- | --- |
| 精确样本、地址和证据分层 | 继续绑定 programmer 哈希、函数 VA、字段来源；引用原始指令和版本差分记录 | 旧 x86 EXE 的 VA、对象偏移和模拟环境不能用于 AArch64 loader |
| 中间值差分方法 | 将“逐字段字节 → 完整消息 → 摘要 → 响应／状态”加入认证验收设计 | 可以复用案例组织方式，不能用旧 token 向量证明新 RSA 协议正确 |
| 边界案例 | ID 短值与尾字节、ASCII／整数区别、终止符、零填充、Y/N 和 W/R、占位摘要、空 NV、挑战变化 | 只有取得对应的合法样本／明确协议后，才形成新协议测试；当前不生成有效签名或改代码 |
| XML／raw 状态与错误处理 | 分开记录日志、进入 raw 的 ACK、完整数据和最终 ACK；失败不得被后续成功覆盖 | 不复制 NCK 子串判成功、固定属性顺序匹配、短写按请求量推进等旧缺陷 |
| 输入与范围核验 | 区分源文件偏移、目标扇区、实际写集合和配置筛选；包清单与运行时写入分开 | OPS 的 FileOffsetInSrc×512 不可套到 rawprogram 的另一字段，也不新增 OPS 支持 |
| 隐含副作用审查 | 后续匹配客户端须检查 param、provisioning、FFU 和结束 power 等是否在镜像表之外执行 | 当前设计不支持 provisioning 等路径，不随参考工具自动引入 |

研究中“缺少一层证据就保留未确认”的做法与当前方向一致。尤其可借鉴其修正短 SN、内嵌 NUL、时间跨秒和堆初值假设的方法；当前 Rand 不是旧 pk，具体预期值必须重新取证。

## 不应迁移的行为及未解决的包缺项

- 旧 sparse 实现某些 DONT_CARE 会并入零缓冲；当前设计明确跳过并保留原内容。迁移对照案例，不迁移写零行为。
- 旧主链可能自动重试、重连、provisioning、写 param、先设置 boot drive 再 patch，或完成后 power off。当前顺序及错误停止规则以[正式设计](../../designs/oplus-edl.md)为准，不因兼容旧客户端改变。
- 旧 `sha256init → 写入 → sha256final` 是条件摘要检查；当前 SVIP／认证初始化的同名命令须按当前 loader 核对。不能用设备返回摘要代替独立介质回读。
- OPS 尾部魔数 `0x7cef` 与当前 SVIP 魔数 `0x7cef0312` 的字面相似不足以说明同一格式。旧 OPS 解密、许可证容器和 RMX3800 的服务 ZIP／MBN 分别处理。
- 旧研究未给出当前 super 定义的正确组装、四个短别名容器的转换规则或 VIP label 适配，因此这些缺口没有因本次对照消失。

## 对当前计划的决定

1. 将本页作为第六阶段的参考交叉验证，迁入上述验收方法和边界案例；旧报告仍作为独立版本证据保存。
2. 当前 RMX3800 主线继续沿 getsigndata、OSIG 消息、公钥、同会话签名响应和权限链追踪。下一份客户端样本必须先证明它消费这些字段并产生匹配响应，再研究签名获取；只有旧 token 生成器或旧服务域名不满足条件。
3. 旧 OnePlus AES token 若以后需要支持，应以对应 programmer 为目标单独验证，不能默认并入当前 RMX3800 连接流程。本轮不实施产品迁移。
4. “完整认证输入已齐”仍为未通过；旧客户端差分 PASS、许可证校验和现有包文件均不能改变此状态。
