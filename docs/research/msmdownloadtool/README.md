# MSMDownloadTool 协议专题

这些文档从版本分析报告中提取协议行为，不代表所有版本或设备都具有相同行为。精确样本身份和验证边界见各专题正文；版本报告和证据入口见[研究索引](../README.md)。

| 专题 | 内容 |
| --- | --- |
| [认证协议](auth.md) | 登录、许可证、设备 token 与版本差异 |
| [售后包解包与解析](ota.md) | ZIP、OPS、XML、项目字段及镜像区间；不覆盖标准 Android payload.bin |
| [刷写协议](flash.md) | 连接、Sahara、Firehose、写入、回读与结束操作 |

官方 OplusFlashTool 与第三方 Anteasy 的补充章节分别限定于其样本。后续 RMX3800 实测见[实刷记录](../../implementation/rmx3800-cloud-flash-20261116.md)，其中全量回读尚未完成。
