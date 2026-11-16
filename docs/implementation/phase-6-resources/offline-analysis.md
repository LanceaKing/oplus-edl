# RMX3800 16.0.5.740 服务包离线适配分析

日期：2026-11-14。结论：包内确有可分析的原厂域名加载器、签名权限表和 super 构建素材，但尚不能直接供当前产品完成 RMX3800 读写验收。缺口包括认证状态接入、VIP 条件下的 program 字段、super 构建及格式、变体选择，以及缺失文件名的处理语义。没有修改产品或 qdlrs 代码，没有枚举、打开或操作设备。

## 范围和证据

服务包为 [RMX3800domestic_11_16.0.5.740CN01_2026041411410326.zip](https://rms11.realme.net/sw/RMX3800domestic_11_16.0.5.740CN01_2026041411410326.zip)。沿用上轮 ZIP 目录和小文件记录，本轮仅通过 HTTP Range 补取加载器、摘要、四个别名候选及部分包内配置；没有完整下载、组装或刷写固件。包内脚本只作文本读取，没有运行，也没有访问其中的内部构建地址。

现有工具按设计列出的固定提交检查，不使用浮动分支：

- OplusEdlTool：`56cbdda540fd8e9e910c1f46b48779aa943575a3`。
- OPlus_EDL_Toolkit：`895678d4bb279703f3f4f0be775460489ea421d2`。
- 本地 qdlrs HEAD 为 `a80cd1e8cd1cd60634b9c5ce9a4c8c96b727c003`，相关 lib.rs／sahara.rs 与该提交无差异。

证据摘要见 [offline-analysis.json](offline-analysis.json)，关键指令见 [loader-auth-disassembly.txt](loader-auth-disassembly.txt)。临时源文件、二进制、解析脚本和完整反汇编在 `runs/phase-6-offline/`。未运行参考工具的设备流程。Binary Ninja 无头加载因许可证校验失败，后改用 pyelftools 解析嵌套 ELF、Capstone 反汇编；以下地址结论来自指令和命令表，不是伪装成反编译结果的字符串推断。

## 加载器认证：已确认与未确认

受检文件 `IMAGES/prog_firehose_ddr.elf` 为 1,568,936 字节，SHA-256 为 `daaddec2d87fa2f30a527f0e3d656044ef9e2fdece8163afde856cb9505e34ea`。它包含多个嵌套 ELF；不能把外层 ELF 的机器类型当作 Firehose 主体架构。本次定位到文件偏移 `0x110b50` 的 AArch64 ELF，按其程序头映射后检查代码。以下地址均为该模块的声明虚拟地址，未验证设备实际加载地址。

| 观察 | 指令／数据依据 | 可以得出的结论 |
| --- | --- | --- |
| 命令分发表 | `0x149cd000` 起的名称／处理函数对；program → `0x149a27fc`，read → `0x149a29e8`，verify → `0x149a1a6c`，sha256init → `0x149a2554` | verify 和 sha256init 确实接入处理函数，并非闲置字符串 |
| verify 有前置状态检查 | `0x149a1a80–0x149a1a88` 读取状态字节并要求其为 1；失败分支 `0x149a1ae4`；通过后进入数据接收及验证调用 | 不能假定发送普通 configure 或任意 verify 文本就完成认证；正确前序状态仍需建立 |
| read 有状态门槛 | `0x149a2a14–0x149a2a34` 读取状态，未满足时仅一个特定模式值可继续，其余转错误路径 | 读取并非天然免认证；实际模式、状态值及该设备会走哪个分支未知 |
| program 有多重门槛 | `0x149a2818–0x149a286c` 检查两个状态和模式；`0x149a28f8–0x149a2954` 在 VIP 开关生效时读取 label 并调用权限检查 | 上传完成或 configure 成功，均不能替代写入权限证据 |
| VIP 表签名认证是条件分支 | `0x149a8cbc` 调用安全状态查询；安全路径进入 `0x149a4cf4` 认证调用，失败在 `0x149a8e0c` 返回错误；非安全路径另行解析表 | 可以确认硬件安全状态影响校验。不能从非安全分支的存在推导量产设备可跳过签名 |
| sha256init 依赖 VIP 状态 | `0x149a255c–0x149a2578` 检查开关，启用时调用初始化函数 | 命令存在及返回逻辑已核实；未证明它接收的表或状态已准备正确 |

安全状态查询函数 `0x149a4c9c` 和签名调用函数 `0x149a4cf4` 通过底层接口取得结果。本轮未逆向全部安全固件、全部模式来源或 Sahara 端验签路径，因此不声称已证明设备的熔丝状态、信任根、全部认证条件、在线账号需求或具体失败码。

### 包内摘要不是任意可互换的 digest／sig

实际读取 `IMAGES/DigestsToSign_nv00000000.bin.mbn`：33,144 字节，SHA-256 `9211d9b8d2d77a5d94f9f4484918a16097ad4cf78e436ab1e6ff27ac0838f075`。这是带 ELF 封装的表，而非仅 32 字节哈希。

其偏移 `0xe8` 的载荷起始魔数为 `0x7cef0312`，与加载器 `0x149a8d4c–0x149a8d60` 的比较值一致。头部给出 32 字节头、195 条记录、132 字节记录长度，满足 `32 + 195 × 132 = 25772`，也对应加载器的长度／乘积校验。记录中的两个字符串字段与分区名和文件名吻合。由此可确认它与该加载器的 SVIP 权限表格式有关；没有验证其数字签名，也没有证明它满足参考工具中两次上传所需的全部输入。

包内另有多个 NV 版本及 GKI_IMAGES 下的同名摘要。`*.sig` 或 `all_files_checksum` 文件的扩展名不能证明它是参考工具界面的 Sig 输入。现有证据没有给出可靠的“programmer + digest + sig”三元组，也没有证明三者可跨版本混配。

### 参考工具的流程与当前产品差异

[OplusEdlTool 主流程](https://github.com/salokrwhite/OplusEdlTool/blob/56cbdda540fd8e9e910c1f46b48779aa943575a3/MainWindow.axaml.cs#L1816)在 configure 未成功时执行：programmer → digest → verify → sig → sha256init → configure。其 [EdlService](https://github.com/salokrwhite/OplusEdlTool/blob/56cbdda540fd8e9e910c1f46b48779aa943575a3/Services/EdlService.cs#L132)使用外部 Sahara／Firehose 工具发送这些输入。注意 `SendDigestsAsync` 虽取得进程结果，却直接返回 true；调用方的布尔成功不能单独证明摘要已被设备接受。

[Toolkit 的 send_loader](https://github.com/snowwolf725/OPlus_EDL_Toolkit/blob/895678d4bb279703f3f4f0be775460489ea421d2/src-tauri/src/firehose_service.rs#L483)也有上述阶段，并在 digest 后增加 transfercfg。它要求外部提供输入；没有在这个入口中为本包生成两份认证文件。两个工具的握手并非完全相同，也都不能替代该版本实机结果。

当前 [device::initialize](../../../src/device.rs)只有 Sahara → welcome → configure；[CLI](../../../src/cli.rs)没有 digest／sig 参数。这是已确认的接入缺口，而不是建议把参考工具的命令不加验证地搬过来。Sahara 上传、厂商验证状态、VIP 表验签和读写权限是不同环节，应分别记录结果。

还有一个独立的协议字段缺口：固定 qdlrs 的 `firehose_program_storage` 仅将 label 用于进度消息，发出的 program XML 不含 label／filename。上述加载器在 VIP 启用分支明确读取 label；缺少该字段会走错误路径。因此即使额外握手完成，也不能据此保证当前 program 请求可用。该结论仅适用于走到此 VIP 分支的组合，不扩大为所有 Firehose 都必须包含这些字段。

## 缺失输入的实际来源和处理

### super.img：包内提供构建输入，不提供最终镜像

三份 `META/super_def.*.json` 都列出 38 个逻辑分区项，其中 19 项带镜像路径；这些路径均可在 ZIP 中央目录找到。其共同声明为：super 大小 15,569,256,448 字节、块大小 4096、alignment 1,048,576、metadata size 65,536、3 个 group 项。这里仅核对定义及目录存在性，未读取全部大镜像、核验展开长度或实际运行构建。

| 定义 | NV 声明 | 关键差异 |
| --- | --- | --- |
| super_def.00000000.json | 00000000／ALLNET | my_manifest 路径实际指向 10010111 版本 |
| super_def.10010111.json | 10010111／ALLNET | 使用 my_manifest.10010111.img |
| super_def.10011101.json | 10011101／CN-CT | 使用 my_manifest.10011101.img |

不能按文件名排序、目录第一项或最后一项推定设备变体。包内 GKI_IMAGES 还提供替换素材，不能与普通 IMAGES 的摘要或内核镜像任意混用。

| 参考路径 | 具体处理 | 不应继承的假设 |
| --- | --- | --- |
| [Toolkit super_image_creater](https://github.com/snowwolf725/OPlus_EDL_Toolkit/blob/895678d4bb279703f3f4f0be775460489ea421d2/src-tauri/src/super_image_creater.rs#L187) | 读取定义；逐镜像调用 simg2img，失败时回退原文件；再用 lpmake 设置逻辑分区／组，输出稀疏 IMAGES/super.img。命令含 virtual-ab，metadata-slots 取 groups.len() | 转换失败不一定意味着输入原本是 raw；group 数不自动等于正确的 metadata slot 数。该函数的实际调用仍走 Windows cmd，不能由工具其他 Linux 功能推定此构建路径已跨平台 |
| [Toolkit 文件检查](https://github.com/snowwolf725/OPlus_EDL_Toolkit/blob/895678d4bb279703f3f4f0be775460489ea421d2/src-tauri/src/file_util.rs#L275) | 取匹配定义列表的最后一项；缺失 super 时保留对应 program 项，随后构建 | 它没有根据实际 NV 选定义；已有 super 时，super 仍在跳过列表，不能把该路径概括成无条件刷写 super |
| [OplusEdlTool SuperMergeService](https://github.com/salokrwhite/OplusEdlTool/blob/56cbdda540fd8e9e910c1f46b48779aa943575a3/Services/SuperMergeService.cs#L141) | 取目录中第一份定义；转换带路径的分区镜像，再调用 lpmake；固定 metadata-slots 为 2，参数不含 sparse／virtual-ab | 缺失或转换失败的输入会被略过，后续按索引把 rawFiles 配回分区；存在错位风险。这个路径的输出不能未经格式核验就配合本包 sparse=true XML 使用 |

OplusEdlTool 还有合并 `super.<序号>.<摘要>.img` 的另一条路径；本包需要根据定义和逻辑分区素材构建，不能混淆为任意碎片拼接。当前产品没有 super 构建器，只能接受已经准备好的 raw 或支持的 Android sparse 镜像；本包 rawprogram0 明确将 super.img 标为 sparse。参考工具提供实现线索，但本轮没有证明其生成结果与原厂最终 super 等价。

### 其他四项：签名表给出候选别名，参考工具选择跳过

上轮“缺失”指 XML 所在目录下不存在字面路径；本轮补充发现如下实际文件。该映射来自同包权限表记录，不仅是按名称猜测。

| XML 字面文件／目标 | 同包表中记录的文件 | 实际字节数 | XML 声明写入字节数 |
| --- | --- | ---: | ---: |
| oplus／ocdt | oplus23631.bin | 68 | 131072 |
| sec.dat／secdata | sec_smt.dat | 12088 | 28672 |
| dynamic_nvbk.bin／oplusdycnvbk | dynamic_nvbk.23631.bin | 599040 | 20971520 |
| static_nvbk.bin／oplusstanvbk_a | static_nvbk.23631.bin | 566272 | 20971520 |

实际读取四个文件：分别有 OCDT 类头、ELF 头、两个 OEMNVBK 头，均不是 Android sparse 魔数。它们比 XML 声明范围短多个扇区。因此改名只会将“路径缺失”变成“源数据不足”；当前 [XML 预检](../../../src/program_xml.rs)只允许末扇区补零，不允许把这些差额整体补零。它们是否需要厂商专用转换、容器处理、特定写入命令或其他映射，尚未核实。本轮不构造替代内容。

[Toolkit 文件检查](https://github.com/snowwolf725/OPlus_EDL_Toolkit/blob/895678d4bb279703f3f4f0be775460489ea421d2/src-tauri/src/file_util.rs#L370)按分区标签跳过 ocdt、secdata、oplusdycnvbk、oplusstanvbk_a，另跳过 persist；不是自动生成或重命名。该逻辑经 `parser_program_xml_skip_empty` → `parse_file_path` 检查文件，缺失项也不会加入上述分区的写入列表。

[OplusEdlTool 解析列表](https://github.com/salokrwhite/OplusEdlTool/blob/56cbdda540fd8e9e910c1f46b48779aa943575a3/MainWindow.axaml.cs#L859)将不存在的字面路径标为 NOT FOUND 并默认不选中；[刷写入口](https://github.com/salokrwhite/OplusEdlTool/blob/56cbdda540fd8e9e910c1f46b48779aa943575a3/MainWindow.axaml.cs#L1753)再次排除这些项。在已检查的这些路径中，没有发现对上述四个别名的自动映射或生成器。它们完成的是选中子集，不是完整重建服务包。

这也解释了“参考工具能开始刷写”与“当前产品拒绝原 XML”并不矛盾：当前产品按用户明确选择的 XML 严格检查非空文件名，没有隐式跳过特定设备分区。若以后选择子集，必须显式说明省略项和实际覆盖，不能改成缺项静默成功。

## 当前适配缺口及验收影响

| 缺口 | 证据强度 | 对第六阶段的影响 |
| --- | --- | --- |
| programmer 之外的认证输入、状态和握手 | 参考源码流程、加载器指令均确认存在；实际设备要求未测 | 尚不能认定连接后就能读写。需先明确可合法取得且匹配的认证输入及失败处理，不以包内任意 sig 替代 |
| VIP program label／权限信息 | 已确认该条件分支及 qdlrs 发包字段差异 | 只补初始化流程仍可能在 program 被拒；未来如修正应验证真实字段和 ACK，不伪造状态 |
| super 构建、输出格式与 NV／GKI 选择 | 定义及路径齐全，构建未执行；参考实现存在选择和错误处理局限 | 真实固件整包验收未就绪；普通镜像／受控 XML 路径可以作为独立目标 |
| 四项别名及短容器的写入语义 | 表中映射和文件头已确认，转换规则未知 | 不可直接改名、补零或放宽范围检查；显式不测试这些分区时只报告子集 |
| 本地 GPT patch 依赖 | 已有本地／DISK 成对条目，未完成目标地址与写后结果等价核验 | 继续保留原计划限制；不能由“相邻成对”升级为已证明整包兼容 |
| 真实 Sahara、安全状态及当前设备布局 | 本轮没有设备证据 | 版本同名、文件可下载和静态分支均不构成实机通过 |

本次不将这些缺口自动加入首版实现范围，也不修改五工作流契约。后续最小准备顺序为：固定设备与变体信息；核实认证输入来源和加载器实际要求；离线准备格式正确的已选镜像／明确 XML 子集；最后在另行授权范围内依次验证连接、读取、受控写入和回读。缺口未解决时维持“资源已找到，RMX3800 实机适配未验证”。
