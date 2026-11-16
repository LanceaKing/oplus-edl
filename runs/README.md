# 运行记录索引

本目录保存本地实验输入、脚本、输出及实机证据。研究结论见[研究索引](../docs/research/README.md)，阶段验收见[实施索引](../docs/implementation/README.md)。目录名和失败日志保留实验发生时的状态，不能仅凭名称判断成功。

## 实机刷写与启动

| 目录 | 用途 |
| --- | --- |
| [live-cloud-flash-recovery](live-cloud-flash-recovery/) | 合并结果、恢复执行、启动选择与 ADB 启动核对；首先查阅 [summary.json](live-cloud-flash-recovery/summary.json) |
| [live-cloud-flash-attempt5](live-cloud-flash-attempt5/) | 恢复执行前的已确认写入和失败位置 |
| [live-cloud-flash](live-cloud-flash/)、[attempt2](live-cloud-flash-attempt2/)、[attempt3](live-cloud-flash-attempt3/)、[attempt4](live-cloud-flash-attempt4/) | 早期尝试，保留失败原因与协议轨迹 |
| [swm-auto-flash](swm-auto-flash/) | 自动匹配云端资产与执行准备 |
| [phase-6-rmx3800-20261115](phase-6-rmx3800-20261115/) | 原包早期连接、读取检查、固件解压内容及复核脚本 |
| [preview-rmx3800-windows-success](preview-rmx3800-windows-success/) | Windows 用户实测相关材料 |

当前实机证据确认指定固件写入 ACK 和 Android 启动；全量回读未完成。完整范围见[实刷记录](../docs/implementation/rmx3800-cloud-flash-20261116.md)。

## 输入准备与产品验证

| 目录 | 用途 |
| --- | --- |
| [firmware](firmware/) | 固件 ZIP、SHA-256 和校验记录 |
| [preview-super-implementation](preview-super-implementation/) | super 构建、两次准备结果与离线验证 |
| [preview-rmx3800-alignment](preview-rmx3800-alignment/)、[preview-export-locations](preview-export-locations/) | preview 行为对齐与导出位置 |
| [cloud-flash-implementation](cloud-flash-implementation/)、[phase-6-offline](phase-6-offline/) | 实现与离线检查记录 |
| [wait-edl-smoke](wait-edl-smoke/)、[egm-code-review](egm-code-review/) | 等待 EDL 检查与代码评审材料 |
| [phase-6-resource-check](phase-6-resource-check/)、[phase-6-protocol](phase-6-protocol/)、[phase-6-msm-compare](phase-6-msm-compare/) | 资源检查、协议提取与对照脚本 |

## 研究工作材料

| 范围 | 目录 |
| --- | --- |
| MSMDownloadTool | [分析工作区](msmdownloadtool-analysis/) |
| 官方工具 | [OplusFlashTool 分析](oplusflashtool-5.9.18-analysis/)、[工具样本](realme-official-tools/)、[ColorOS Assist](coloros-assist-1.4.42/) |
| 第三方工具与云端资产 | [Anteasy preview](anteasy-oplustool-preview/)、[云端 loader](anteasy-cloud-loader/)、[OplusMuTools](oplusmutools-v22/)、[Windows 验证准备](oplusedltool-windows-validation-20261115/) |
| RMX3800 会话与密码处理 | [会话授权](rmx3800-session-auth-20261115/)、[getsigndata](rmx3800-getsigndata/)、[loader crypto](rmx3800-loader-crypto/)、[secure auth](rmx3800-secure-auth/)、[早期授权](phase-6-auth/)、[外部指引](ada-guides-20261115/) |

根目录散落的两个固件样本归档到 [root-package-samples](root-package-samples/README.md)，来源与用途尚未确认，哈希单独保存。

## 清理与复用

清理明细见 [cleanup-manifest.json](cleanup-manifest.json)。两份 super.img 经完整 SHA-256 核对相同，保留 `preview-super-implementation/prepared-v2/super.img`，旧 `prepared/super.img` 改为指向它的相对符号链接。两份 selection.json 和 rawprogram XML 独立保留。

这两个 super 路径现在读取同一份内容。历史产物只用于读取；需要重新生成时使用新的输出目录，避免覆盖共享的证据文件。目录搬迁或复制时应一并保留链接目标。

Python 字节码缓存与 Finder 杂项已删除。固件原包、解压输入、分析数据库、执行轨迹、失败样本和脚本均保留；`.partial` 文件可能是采集证据，不按后缀直接删除。此清理没有运行设备命令或重做实机验证。
