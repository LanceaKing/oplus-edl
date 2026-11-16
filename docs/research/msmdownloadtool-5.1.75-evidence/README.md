# MSMDownloadTool 5.1.75.0 续析证据

对应[专题报告](../msmdownloadtool-analysis.md#v5175)与[原报告第 13 节](../msmdownloadtool-analysis.md#v4028)。主样本原始 SHA-256 为 `0ac4005251f060cc759b251fcbb52a07d146c54bd8e4f4eb62092c70f28dc0b6`。

- `unpack-manifest.json`：四组件源/输出哈希，16 个 LZMA 块位置、大小和哈希。
- `main/hlil/`、`main/asm/`：Binary Ninja GUI API 的原始文本，文件名为实际函数/续段 VA。
- `mes/asm/`：DLL 受保护导出入口。没有用缺失的 HLIL 假装实现已恢复。
- `function-evidence.json`：原始 BN 名称、地址、文本路径、行数。库签名误识别和未解析调用仍保留，报告不依赖这些名字推断行为。
- `constants.json`：恢复节中用于支持关键字段、命令和端点的直接字节。
- `restored-delay-imports.json`、`mes-exports.json`：主程序的延迟导入与 DLL 导出。
- `digest-vectors.json`：隔离 MD5 原语执行结果，不是登录测试。
- `imports-attempt.*`、`emulation-attempt.*`：有限时长导入恢复及模拟加载尝试，包括失败结果。
- `provenance.json`：工具来源、脚本哈希、是否执行进程/网络/设备的范围记录。
- `acquisition-op8.json`、`pe-metadata.json`：原始采集记录的副本；后者同时含旧版资料，以文件名和哈希区分。
- `SHA256SUMS`：本目录全部证据文件（除清单自身）的哈希。

主 EXE 地址基准为 `0x400000`，DLL 为 `0x10000000`。文本来自压缩层恢复后的文件，不能直接对原文件按 RVA 作磁盘偏移读取。应先核对清单里的源哈希，再解压并核对恢复文件哈希。

本机[工作目录](../../../runs/msmdownloadtool-analysis/op8-unpack/)保留原始工具代码、复现与验证脚本、完整请求/响应记录、恢复 PE 和两个 BN 数据库。第三方源码保留在该工作目录并记录来源，不复制为本仓库的产品实现。

复核命令见专题报告第 10 节。`reproduce_unpack.py` 默认只在内存中重建并比较；`check_digest.py` 只执行独立摘要函数；`validate_evidence.py` 检查 16 个块、哈希、关键指令和文档链接。所有这些通过都不证明真实授权或刷写成功。

新增 `devirt/` 记录：

- 两组件共 1,480 个导入槽的解码元数据、名称密钥，以及 36 个 API 转移跳板和 3 个函数指针装载跳板的停止式求值。
- 有界 VM 跟踪及 MES 保护层 CRC 处理器记录；零地址退出不是业务授权结果。
- 两把不同的 RSA 公钥、设备 token 参考常量对应关系和固定参考源码哈希。
- 许可证大端头解析与两个 AES-128-CBC 原语向量；不包含真实授权文件或有效设备 token。

`main/` 已补入许可证申请、签名验证、解密、JSON、时间/HMAC 与文件保存的 BN 文本。总索引现为 286 份函数文本，其中主程序 HLIL 为 168 份。`devirt/provenance.json` 区分原始映像执行与仅在模拟器中进行的导入哨兵重定位。

新增 `devirt2/` 记录：

- 两类 VM 槽操作的 22 个原生变体，及 444 次已记录状态转移的解码比较。
- 六次未成功返回的 VM 跟踪；每次只保留前 500、后 400 个分派候选状态及计数，不是完整 VM CFG。
- 三组 token 输入复制和八组 PCBA 门槛组合的机器码执行结果；均停止在业务计算或外部调用之前。
- MFC 消息表、PCBA 格式串、MES API 名称初始化与函数文本。
- `provenance.json`：映像、脚本和证据哈希，以及合成初始化与监视范围的限制。

最新结论和剩余缺口以专题报告第 13 节为准。三类设备 token 的已测中间与输出链已恢复；完整进程初始化、全部输入边界、设备接受条件和 MES DLL 请求／响应状态机仍未验证。

新增 `devirt3/` 记录：

- 原零环境的 4,977 次缓存比较、第三类 VM 寄存器分配、完整分派流哈希与选取窗口。
- 六组运行时字段控制、五组否定性实验和 19,159 项 CRC 表复算。
- 20 组 token 数据流案例，其中 11 组完整输出通过 SHA-256、原生 PRNG 和独立 AES 复算；另有三组堆初值控制。
- `traces/` 保存原始执行 JSON 的摘要、关键原生调用及缓冲区；每份注明原始文件哈希与删选范围。
- `provenance.json` 说明六种 API 模型及运行时数据；原始跟踪、复现脚本和完整基线流仍在工作目录。
