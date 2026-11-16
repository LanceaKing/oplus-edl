# 研究资料索引

本目录保存工具行为分析、协议专题及原始证据。产品完成状态见[实施索引](../implementation/README.md)，代码评审见[评审索引](../../reviews/README.md)。静态分析、离线执行与实机结果分别以各自记录为准。

## RMX3800 当前进展

后续使用 preview 云端配套资产完成了 RMX3800 指定固件的写入与启动核对：351 次 program、65 次 DISK patch 获 ACK，ADB 确认 Android 14 / RMX3800_14.1.0.190(CN01) 启动完成。全量回读仍未完成。详见[实刷记录](../implementation/rmx3800-cloud-flash-20261116.md)及[结果摘要](../../runs/live-cloud-flash-recovery/summary.json)。

| 阅读顺序 | 文档 | 用途 |
| --- | --- | --- |
| 1 | [preview 路径对齐](preview-rmx3800-alignment.md) | 云端 loader、digest、sign 与产品接入；阶段性状态由后续实刷记录补充 |
| 2 | [云端资产记录](swm-live-cloud-assets.json) | 本次配套资产身份与哈希 |
| 3 | [原包会话授权核查](rmx3800-session-authorization.md) | 早期原包挑战、验签与签发请求结果；不是当前所有路径的可用性结论 |
| 4 | [Anteasy OplusTool 分析](anteasy-oplustool-4.2026.009-analysis.md) | 第三方工具的设备认证、云端服务与持久记录分析 |
| 5 | [官方 OplusFlashTool 分析](oplusflashtool-5.9.18-analysis.md) | 官方 5.9.18.0 的 RCSM、项目匹配及刷写流程 |

原包 programmer、第三方云端 loader 和官方 Windows 工具属于不同验证对象。后续云端路径成功不改变早期原包拒读结果，也不证明官方工具或所有机型均可用。

## 按问题查阅

| 问题 | 专题 |
| --- | --- |
| 认证、许可证、设备 token | [认证协议](msmdownloadtool/auth.md) |
| 固件 ZIP、OPS、XML 与镜像解析 | [售后包解包与解析](msmdownloadtool/ota.md)；其中 OTA 指报告中的售后包，不是 Android payload.bin |
| Sahara、Firehose、写入、回读与结束操作 | [刷写协议](msmdownloadtool/flash.md) |
| PC OTA 助手下载与安装请求 | [ColorOS Assist 1.4.42](coloros-assist-1.4.42.md) |

## 按样本查阅

| 工具与版本 | 报告 | 配套证据 |
| --- | --- | --- |
| MSMDownloadTool 4.0.28.0 | [分析](msmdownloadtool-analysis.md#v4028) | [目录说明](msmdownloadtool-4.0.28-evidence/README.md) |
| MSMDownloadTool 5.1.75.0 | [续析](msmdownloadtool-analysis.md#v5175) | [目录说明](msmdownloadtool-5.1.75-evidence/README.md) |
| MSMDownloadTool 3.0.2.0 / 5.0.15.0 / 5.1.77.0 | [多版本分析](msmdownloadtool-analysis.md#multi) | [证据目录](msmdownloadtool-multi-version-evidence/) |
| 官方 OplusFlashTool 5.9.18.0 | [分析](oplusflashtool-5.9.18-analysis.md) | [证据目录](oplusflashtool-5.9.18-evidence/) |
| 第三方 Anteasy OplusTool 4.2026.009 | [分析](anteasy-oplustool-4.2026.009-analysis.md) | [证据目录](anteasy-oplustool-4.2026.009-evidence/) |
| ColorOS Assist 1.4.42 | [分析](coloros-assist-1.4.42.md) | 原始材料位置见报告的“样本与分析文件” |
| RMX3800 原包会话 | [核查](rmx3800-session-authorization.md) | [证据目录](rmx3800-session-authorization-evidence/) |
| RMX3800 preview 接入 | [对齐](preview-rmx3800-alignment.md) | [证据目录](preview-rmx3800-alignment-evidence/) |

## 资料维护

报告及证据保留原路径，便于已有文档、脚本和哈希清单继续引用。`*-evidence/` 中的反汇编、HLIL、JSON、日志及验证脚本属于研究证据；体积较大不表示可以删除。原始导出和后续整理可能含相同片段，应按来源与用途区分。

旧报告中的“尚未验证”“没有操作设备”描述当次研究范围。新增进展通过后续记录链接补充，不改写原始实验结果。报告日期沿用各自注明的时区。
