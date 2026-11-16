# RMX3800 云端资产实刷记录（2026-11-16）

已使用自动下载的 profile 11 loader、digest、sign 实刷 `RMX3800domestic_11_14.1.0.190CN01_2025012302300313.zip`，NV ID 为 `10010111`。主机先等待 EDL，再独立执行 `adb reboot edl`。产品 flash 不调用 ADB。资产哈希见[资产记录](../research/swm-live-cloud-assets.json)。

57 个选中 XML 条目展开为 351 次 program，共 13,201,825,792 字节，全部取得最终 ACK。65 次 DISK patch 取得 ACK；另外 65 个本地文件 patch 未作为设备命令执行。未完成全量读回校验；恢复报告返回 VerificationIncomplete（退出码 5），不能把 ACK 当作逐字节验证。

第五次连接的前 7 个 program 已确认，super 首次命令被 NAK，未传输该命令的数据。随后在同一 Firehose 会话执行原工具的 RwMode 只读探测，从 super 继续剩余已知未执行条目。恢复使用生产 Writes 实现和 runs 下的专用入口；产品没有自动断线续写。

首次 reset 后仍出现 Sahara。显式设置启动 LUN1 并再次 reset 后，USB 切换到 `18d1:4ee8`。设置前后的 Boot Partition Enabled 读回均为 1，因此不能认定首次返回 EDL 是由启动选择值 0 导致。2026-11-16，用户确认手机已进入初始设置，刷后启动界面验证通过。用户完成设置并授权 ADB 后，实读设备 f2d6521b 的 sys.boot_completed=1、dev.bootcomplete=1，ro.build.display.id=RMX3800_14.1.0.190(CN01)，Android 14，型号 RMX3800，当前槽位 _a，增量版本 U.1a3c3c8_1-1d15d。启动完成状态和目标固件版本均已核对，原始输出保存于 runs/live-cloud-flash-recovery/boot-verification.log。全量回读仍未完成。

产品补齐 DIGEST table ACK、verify rawmode ACK、sign USB 结束包及最终 ACK；收到 VIP 请求后立即发送摘要表。写入前执行 preview 的 BackupGPT 只读模式探测，成功后使用原工具的 filename、label 和 special_rw_mode，保留实际物理范围及报告分区身份。指定 preview UFS 启动 LUN 时，写入前核对 Boot LUN ID 为 1 或 2；普通数据 LUN 或缺失信息均停止。

Binary Ninja 地址和修正证据见[分析记录](../research/preview-rmx3800-alignment.md)。本地原始记录为 `runs/live-cloud-flash-attempt5/` 和 `runs/live-cloud-flash-recovery/`；后者的 `result.json`、`stderr.log`、`boot-repair.log` 和 `summary.json` 分别保存执行结果、设备日志、启动选择读回和合并计数。本次结果只适用于记录中的输入。

最终离线检查：格式检查、全目标全功能 Clippy、122 个常规测试及独立 JSON stdout 子进程检查通过，1 个需显式调用的云端探测测试未运行。这些测试不访问手机，不能代替启动验收。
