# MSMDownloadTool 静态分析证据

对应[分析报告](../msmdownloadtool-analysis.md#v4028)。主样本 FileVersion 为 4.0.28.0，SHA-256 为 `ea4f8ae38e2feb09def08ff5be35c563376b1773cfc64b95b1128e994e0387c8`。

## 内容

| 文件 | 来源与用途 |
|---|---|
| `acquisition-op6.json` / `acquisition-op8.json` | HTTPS Range 提取记录、ZIP 成员信息和样本哈希；不是整包哈希 |
| `archive-*.headers` / `range-op6-last.headers` | 实际 HTTP 响应头；只保留归档元数据和最后一次 Range 响应，不是全部请求日志 |
| `pe-metadata.json` | pefile 对下载字节的独立解析，含 5 个 PE 的版本、节、导入、资源、证书目录 |
| `bn-*-metadata.json` | Binary Ninja 版本、入口、架构、自动函数数量 |
| `function-map.json` | 90 个精选业务函数的地址、BN 自动名称、调用关系及 HLIL 路径 |
| `hlil/` | BN GUI Python API 自动导出的 HLIL，逐行带地址；未人工改写 |
| `disassembly/` | BN MCP 返回的关键函数汇编；部分文件外层仍是工具的 JSON 文本 |
| `profile-continuation.txt` | 因错误 noreturn 假设丢失的配置分发续段，临时在 `0x415656` 建入口后取得 |
| `dialog-manifest-resources.json` | PE 元数据的对话框/manifest 子集；不是完整的控件结构解析 |
| `supplemental-data.json` | 直接从主 EXE 映射 VA 读取的短路径、模式表和算法常量 |
| `fetch_sample.py` | 获取脚本快照；参数为 URL、新的输出目录、可选的成员过滤开关 |
| `bn_export_op6.py` | BN GUI 控制台导出脚本快照；使用当前 `bv`，含本机绝对输出路径 |
| `SHA256SUMS` | 此目录所有文件的 SHA-256，清单自身除外 |

BN 自动名称/类型可能误识别，不代表这些函数具有对应库函数语义。调用图也受间接调用和函数边界识别影响。报告结论以地址、调用上下文和必要的汇编复核为依据。

`profile-continuation.txt` 的临时函数边界不在最早导出的基线 `.bndb` 中；它只是帮助观察原函数续段，未改变 EXE 字节。不要把基线自动导出的 `0x415300` 当成该函数的全部可达代码。

## 复核

在本目录执行：

```sh
shasum -a 256 -c SHA256SUMS
```

报告和大文件位于：

- 报告：`docs/research/msmdownloadtool-analysis.md`（v4028 部分）。
- EXE、`.bndb`、全量 14,068 个自动函数导出：`runs/msmdownloadtool-analysis/op6/`。
- 新版受保护样本与数据库：`runs/msmdownloadtool-analysis/sample/`、`runs/msmdownloadtool-analysis/bn-MsmDownloadTool_V4.0/`。

EXE 和数据库没有复制进本文档目录。所有行为证据均来自静态分析；没有执行 EXE、操作设备或发送厂商服务请求。取得的成员通过 ZIP CRC 校验并计算哈希，不意味着社区镜像已被厂商认证。
