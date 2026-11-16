# 16.0.5.740 认证输入与参考工具握手追踪

后续[当前原包授权核查](../../research/rmx3800-session-authorization.md)新增真实挑战对齐及第三方签发入口的一次失败请求，并区分 14.1 与本报告 16.0 loader 的前序门槛。下文结论继续限定为所列样本和参考提交。

日期：2026-11-15（Asia/Shanghai）。结论：**目前不能用已检查的服务包文件组成一套已确认完整、可供该加载器使用的认证输入。** 包内有 programmer 和 SVIP 表候选，但缺少与该加载器会话验证对应的已确认签名材料、取得方式及完整交互依据。两个参考工具只消费用户选择的 digest／sig，并不提供这些材料或证明它们适配 RMX3800。

本轮只做静态文件、源码和指令分析，匿名分段补取小文件及发布包目录。没有运行下载到的程序，没有枚举、打开或操作设备，没有修改产品和 qdlrs 代码。没有签发、伪造或发送认证数据。

后续纯离线追踪见[会话协议分析](session-protocol.md)：已细化 getsigndata 日志、208 字节消息布局及固定公钥。本文所称会话字段实际为 16 字符 Rand 文本；成功状态 1／2 对应消息中的 Y／N，不是待验消息内的数值。签名取得流程仍未补齐。

## 证据范围

加载器和服务包沿用[前次分析](offline-analysis.md)的精确版本及哈希。参考源码仍固定为 OplusEdlTool `56cbdda540fd8e9e910c1f46b48779aa943575a3` 与 Toolkit `895678d4bb279703f3f4f0be775460489ea421d2`。本轮新增检查它们的输入选择器、Toolkit 前端到后端的数据传递、内置／外部后端分支及随仓库提供的 fh_loader Linux 二进制。

另检查了 [Toolkit V0.9.7 Windows portable](https://github.com/snowwolf725/OPlus_EDL_Toolkit/releases/tag/V0.9.7) 和 [OplusEdlTool OplusEDLTool_20260912](https://github.com/salokrwhite/OplusEdlTool/releases/tag/OplusEDLTool_20260912) 的 ZIP 目录。这只是发布资产是否携带文件的检查，不把它们的运行行为等同于上述固定提交。

结构化结果、文件 SHA-256、发布资产 URL 和条目见 [authentication-inputs.json](authentication-inputs.json)。加载器关键指令见 [authentication-flow-disassembly.txt](authentication-flow-disassembly.txt)。临时原始资料位于 `runs/phase-6-auth/`，未提交或推送。

## digest／sig 从哪里来

| 工具与入口 | 实际来源 | 缺少的来源链 |
| --- | --- | --- |
| OplusEdlTool | [三个文件选择器](https://github.com/salokrwhite/OplusEdlTool/blob/56cbdda540fd8e9e910c1f46b48779aa943575a3/MainWindow.axaml.cs#L446)把用户所选本地路径写入 DevPrg、Digest、Sig；发送时直接读取这些路径 | 此路径没有按 RMX3800／NV 自动匹配、从固件生成签名、下载认证文件或访问签名服务的实现 |
| Toolkit | [App.vue](https://github.com/snowwolf725/OPlus_EDL_Toolkit/blob/895678d4bb279703f3f4f0be775460489ea421d2/src/App.vue#L144)默认显示 res/devprg、res/digest、res/sig；[选择器](https://github.com/snowwolf725/OPlus_EDL_Toolkit/blob/895678d4bb279703f3f4f0be775460489ea421d2/src/composables/useConfigPanelEventHandler.js#L8)仅替换路径，[发送按钮](https://github.com/snowwolf725/OPlus_EDL_Toolkit/blob/895678d4bb279703f3f4f0be775460489ea421d2/src/composables/useOperationEventHandler.js#L154)原样传给 Rust | 默认路径不是内置文件；固定提交的 res 目录没有 devprg、digest、sig，也没有在这个流程中生成它们 |
| 已查发布资产 | 两个 Windows ZIP 都包含外部工具；Toolkit 另有握手 XML | 目录中均未附带目标 programmer、digest 或 sig。仅排除已查资产，不能推广为所有历史发布包或社区附加包都没有 |

Toolkit 的 `edl_loader_util.rs` 只做加载器字符串识别、证书候选扫描及根证书摘要提取，没有签名生成器。识别到同一 SoC 或证书摘要不能补齐授权签名。上面没有找到来源，不等于这些材料在其他渠道绝对不存在。

两种 UI 的 Digest／Sig 标签是用户界面命名，不是协议中的可靠类型声明。二者在外部路径都以 `--signeddigests=<用户路径>` 交给 fh_loader，不能仅凭参数同名就认定两个文件内容相同或可互换。

## 实际握手及成功判断

| 路径 | 固定源码中的动作顺序 | 结果局限 |
| --- | --- | --- |
| OplusEdlTool 手动进入 Firehose | programmer → 可选 digest → verify → 可选 sig → sha256init → configure | 此入口仅强制 programmer 非空；允许省略文件只是 UI 分支，不证明设备允许省略 |
| OplusEdlTool StartFlash | 先要求三个路径非空；configure 尝试未成功时，执行 programmer → digest → verify → sig → sha256init → configure | configure 已成功时跳过上传序列；不能把此分支解释为已验证所有后续权限 |
| Toolkit 外部 Windows | programmer → digest → transfercfg → verify → sig → sha256init → configure | 内层依进程错误停止，但外层 `send_loader` 丢弃 Result，随后发出已加载状态并返回 OK |
| Toolkit 外部 Linux | programmer → digest → verify → sig → sha256init | 与 Windows 不同：没有同位置的 transfercfg 和最终独立 configure；各调用参数也不同，不能合并为同一已验证握手 |
| Toolkit native=true | 创建 SaharaClient，查询芯片信息并调用 send_loader | 该分支没有消费 digest／sig 参数；本次没有把依赖内部实现扩展成完整认证证明 |

源码依据：[OplusEdlTool 手动入口](https://github.com/salokrwhite/OplusEdlTool/blob/56cbdda540fd8e9e910c1f46b48779aa943575a3/MainWindow.axaml.cs#L1086)、[StartFlash](https://github.com/salokrwhite/OplusEdlTool/blob/56cbdda540fd8e9e910c1f46b48779aa943575a3/MainWindow.axaml.cs#L1738)、[Toolkit 外部握手](https://github.com/snowwolf725/OPlus_EDL_Toolkit/blob/895678d4bb279703f3f4f0be775460489ea421d2/src-tauri/src/firehose_service.rs#L483)、[Toolkit 分派与结果](https://github.com/snowwolf725/OPlus_EDL_Toolkit/blob/895678d4bb279703f3f4f0be775460489ea421d2/src-tauri/src/lib.rs#L424)。

还须保留两个边界：OplusEdlTool 的 SendDigestsAsync 取得进程返回后直接返回 true；Toolkit 的普通进程包装器只检查退出码，不解析设备 ACK 的完整语义。因此不能把界面“已加载”或包装器的布尔值当作认证验收标准。当前产品需要保留原始阶段结果，不复制这些错误传播缺陷。

### fh_loader 不替用户签名

静态检查固定 Toolkit 仓库提供的 Linux fh_loader，SHA-256 `0123d63241835c52ababdce160c42335bc46b15d766a9f624320434c79459a38`，没有执行它。该 ELF 保留符号：`SendSignedDigestTable` 位于 `0x11482`。已检查的路径打开给定文件、读入发送缓冲、检查读取长度，再在 `0x117cb` 调用 `sendTransmitBufferBytes`，随后处理设备响应。这个路径不是从私钥生成签名，也没有将 102 字节的 DER 签名转换成该加载器的会话响应。

此二进制结论只适用于该 Linux 文件；Windows 版本未逐指令核验。GUI 对它们的调用均是文件传递，不能从 `--signeddigests` 这个选项名称推导出“工具能替我生成缺失认证数据”。

## 包内候选逐项判定

本轮重新取得并检查全部 6 份 DigestsToSign、两个 `.sig`、两份 sign_info、GKI 哈希清单和 Setting.xml；没有只按扩展名判断。

| 输入 | 已确认的内容 | 能否直接认定为完整认证材料 |
| --- | --- | --- |
| IMAGES/prog_firehose_ddr.elf | 与前次哈希相同的嵌套 ELF programmer | 只能确认文件身份；Sahara 接受和运行状态未测 |
| IMAGES/DigestsToSign_nv00000000.bin.mbn、nv10010111 | 两者字节完全相同；33,144 字节，SHA-256 `9211d9b8d2d77a5d94f9f4484918a16097ad4cf78e436ab1e6ff27ac0838f075` | SVIP 表候选；不是已确认的会话签名响应 |
| IMAGES/DigestsToSign_nv10011101.bin.mbn | 同尺寸，但 SHA-256 `73ed6533bd57620b650ccdade5adca2584d8eceb9a16e54422f9a542e77ce188` | 必须与实际 NV／输入组合对应，不能只因尺寸一致而替换 |
| GKI_IMAGES 下三份 DigestsToSign | nv00000000 与 nv10010111 相同，SHA-256 `6d8e8a57391bb9829cd72125198212ff24e1c197c6d2cc1824d2688bba9d2be1`；nv10011101 为 `c4a3f2d78b44a982350aa81670c4d80f11a3d1ea851df829641acc8dde418d16` | 与普通 IMAGES 的表不同，不能把相同文件名当作相同内容 |
| all_files_checksum.txt.sig | 102 字节，DER SEQUENCE 内两个 48 字节 INTEGER | 结构类似常见椭圆曲线签名编码；未取得公钥并验证，不能确认算法／签名对象，更不是直接可用的 256 字节 RSA 验证输入 |
| GKI_IMAGES/hash_128_files_all.txt.sig | 103 字节，DER 内两个 INTEGER，长度 49／48（首个含前导零） | 同样不能直接充当加载器会话验证材料；不是 SVIP ELF 表 |
| sign_info.txt、GKI_IMAGES/sign_info.txt | 仅记录构建签名返回值的短文本 | 不是密钥、签名响应或摘要表 |
| IMAGES/Setting.xml | SAHARA 指向 image ID 13 的 programmer；含 DownloadConfigSigned、ImgSHA256Check 等设置 | 没有提供 GUI digest／sig 的配对路径或会话签名来源。BasicInfo Project=22825，与 Projectconfig／build 信息的 23631 不一致，按模板／元数据歧义保留，不直接当作设备身份 |

六份表均有同一类 SVIP 载荷结构：魔数 `0x7cef0312`、32 字节头、195 条记录、每条 132 字节，与已分析的加载器格式检查相符。这证明格式联系，**不证明表签名已验证、设备信任它或它能同时承担 GUI 的两个输入角色**。相同 NV 文件仅在上述两对中确认字节相同。

## 加载器进一步追踪：表验签与会话验证是两件事

以下仍为 SHA-256 `daaddec2…05e34ea` 的 AArch64 嵌套模块，偏移 `0x110b50`，地址为声明虚拟地址。未使用真实设备状态。

1. **SVIP 表载入。** `0x149a47a8` 在表尚未初始化的路径接收数据并调用 `0x149a8c9c` 校验／解析表，之后更新表相关状态。该表与包内 DigestsToSign 的结构相符。这一层不能替代后面的会话验证。
2. **getsigndata 建立前置状态。** 命令处理函数 `0x149a19b8` 准备芯片／会话数据，经若干子调用后，将成功状态写到 `0x8025b280`（写指令 `0x149a1a44`）。本次模块扫描中，找到的直接写入者为该处理路径；没有证明所有启动路径、其他模块或间接写入均不存在。
3. **verify 检查前序。** `0x149a1a80–0x149a1a88` 要求上述状态为 1，否则走序列错误分支。已检查的两个工具自动序列没有显式发送 getsigndata。因此不能将其握手直接认定为当前原包加载器的完整流程；工具可能针对不同加载器或已有会话状态，但本轮没有相应证明。
4. **verify 还要接收签名数据。** `0x149a1414` 检查接收缓冲至少 4096 字节，调用接收函数并要求得到 4096 字节；随后将缓冲开头交给 `0x149a1224`，长度参数为 256。后者组装 208 字节待验消息，纳入当前状态中的会话字段、模式和摘要字段，并调用 `0x149a4610` 的 RSA 验证路径。成功后才设置读写所用的验证状态。GUI 下一条“发送 sig”与这里等待原始数据的时序有联系，但尚无完整抓包证明它所需的文件布局。
5. **不能用静态清单签名代替。** 两个包内 `.sig` 既不是 4096 字节传输材料，也不是 256 字节 RSA 签名；包内 SVIP ELF 则属于另一种结构。已检查的文件发送路径没有自动转换／签发步骤。不能把它们重命名、拼接或补零后声称认证输入齐全。

会话字段的日志包含 Rand 等名称，数据经运行时路径取得；本轮未证明其全部随机源、绑定规则或有效期。RSA 调用和字段参与验证已查到，但没有完整恢复所有模式、公钥选择和服务端协议。因此目前只能确认“缺少与该会话验证相匹配的材料和获取流程”，不能断言所有设备必需在线账户，也不能承诺存在可离线复用的通用 sig。

## 仍缺什么

| 项目 | 当前状态 | 验收前需补齐的具体证据 |
| --- | --- | --- |
| programmer | 文件已确认 | 目标设备接受及进入对应 Firehose 的实际结果，留到授权实机阶段 |
| SVIP 表选择 | 有四种不同内容的候选 | 明确当前设备 NV、GKI／普通镜像组合及表对应关系；保留验签结果，不只看文件存在 |
| 第一阶段 digest 的角色 | 仅确认 GUI 把用户文件交给 signed-digest 发送器 | 该位置应发送哪份表或其他材料、匹配哪个 loader、预期响应；不能默认按名称填入 |
| 会话签名输入／Sig 来源 | 已查包及工具发布资产没有一套可确认匹配的材料；没有找到获取／签发实现 | 合法来源、精确文件格式与传输长度、签名绑定的会话数据、有效期／可重用条件。缺的是验证依据和可用响应，不是要求用户取得私钥 |
| getsigndata／verify 时序 | 加载器有前序状态和接收分支，参考自动流程未覆盖完整链 | 先后顺序、ACK／raw 数据边界、异常停止点及其他模式是否已有合法初始化状态 |
| sha256init 及后续权限 | 表和验证状态均可能影响结果 | 明确表初始化成功、verify 成功、权限就绪分别如何观察；不能仅以 configure ACK 收尾 |
| 当前产品接入 | 只有 Sahara → welcome → configure，无相应认证参数／状态流程 | 是否纳入后续实现须另行定范围；即使补握手，前次发现的 VIP label 缺口仍独立存在 |

对第六阶段的决定：把“完整认证输入已备齐”维持为未通过。可以继续只读检查合法来源的额外样本或厂商协议资料；不把当前六份表和两个 `.sig` 任意配成一个三元组，不借用参考工具的 OK 状态宣布适配成功，也不因此修改产品。原先 super、别名短容器、布局和恢复基线的缺口仍然有效。
