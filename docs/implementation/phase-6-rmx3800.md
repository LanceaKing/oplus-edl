# 第六阶段：RMX3800 实机检查与签名摘要表接入

> 历史记录：本文描述当次计划或检查。后续已完成实刷与启动核对，全量回读仍未完成；当前进度见[实施索引](README.md)和[实刷记录](rmx3800-cloud-flash-20261116.md)。

日期：2026-11-15（Asia/Shanghai）。

本次已访问用户指定的 RMX3800，并验证指定服务包的原始 programmer 和签名分区摘要表。设备接受了两者，configure 和 getsigndata 返回 ACK；随后 GPT 读取返回 `download is not allowed.`，没有最终响应，主机等待超时。**未执行刷写，未发送 program、patch、erase、writever 或启动单元修改。** 本记录不将连接完成或摘要表 ACK 计为设备授权、备份或刷写成功。

用户明确授权继续开发并通过 EDL 刷写指定 ZIP，随后确认暂无额外授权方式或已验证 loader，先验证公开资源。本文替代此前准备记录的“未操作设备”状态；历史研究结论继续按其原样本版本适用。

## 实际组合

| 项目 | 本次值 |
| --- | --- |
| 设备 | RMX3800 / RE5C4FL1；ADB 序列号 f2d6521b；项目 23631 |
| 原系统 | RMX3800_16.0.5.740(CN01)，Android 16，安全补丁 2026-04-01 |
| 原启动状态 | verified boot green，bootloader locked，槽位 `_b`，电量 100% |
| NV | `ro.build.oplus_nv_id=10010111`；getsigndata 的 NV 记录也包含此值 |
| 目标服务包 | RMX3800domestic_11_14.1.0.190CN01_2025012302300313.zip |
| ZIP | 11,140,209,208 字节；369 项；重新计算 SHA-256 并完整解压校验 CRC |
| ZIP SHA-256 | `cb5a18e4e28b7ccc705f23384de23f0fd0b9574057c0105d515bb256069e83ec` |
| programmer | IMAGES/prog_firehose_ddr.elf；1,558,188 字节；SHA-256 `9e9f62830e2442fdfd754ef67238f9719b278defd4b4d545a2d03f592ecc3734` |
| 签名表 | IMAGES/DigestsToSign_nv10010111.bin.mbn；33,144 字节；SHA-256 `eeb09e3f8370e657f470a575735deb5e8b94ecffbd7195cd5efd36e72d3c03aa` |
| 主机与接口 | macOS arm64；qdl USB / nusb；05c6:9008，物理位置 key `00:1` |
| 固定协议依赖 | qdlrs `a80cd1e8cd1cd60634b9c5ce9a4c8c96b727c003`，未修改依赖或切换 path patch |

包内 programmer 的运行日志为 `SM8650_V1.0.25`，构建时间 `Jan 24 2024 @ 15:19:41`。它与此前研究的 16.0.5.740 programmer 哈希不同。本次的实机结论只适用于上表组合。跨 Android 16 → 14 的降级启动、回滚策略和数据兼容性没有得到验证。

## USB 阻塞及恢复

最初两次 `adb reboot edl` 后，IORegistry 出现 `QUSB_BULK_CID:0448_SN:F2D6521B`，但节点没有 registered/matched 状态。程序枚举不到候选。macOS 内核日志明确记录：

```text
device functionality blocked by transport restrictions
device will not be registered for matching
```

当前系统设置为“新增配件时询问”，用户未看到配件提示。用户同意临时改为“解锁后自动允许”，并在系统窗口完成身份认证。修改后 9008 成功注册，程序取得唯一候选 `00:1`。本次设备检查结束后，已通过系统设置恢复“新增配件时询问”并读回确认。没有永久关闭配件访问限制。Apple 对该配件许可机制的说明见[官方支持页](https://support.apple.com/en-us/102282)。

## 实际协议结果

临时探测程序位于 [runs 中的 Rust 探测源码](../../runs/phase-6-rmx3800-20261115/probe/src/main.rs)。它复用固定 qdlrs 和产品配置／GPT 代码，保存主机收发字节；不调用分区写入函数。

| 记录 | 动作和结果 |
| --- | --- |
| probe-01 | 轮询枚举，没有可打开的 EDL 候选；未打开设备 |
| probe-02 | 收到 Sahara 3 hello；序列号命令响应长度为 8，初始脚本只接受 4，停止在 ExecuteData 之前 |
| probe-03 | 按上一条完整响应继续取得 8 字节 `1b52d6f248040000`；旧 HWID 命令收到 EndOfImage 类型响应，停止；未上传 loader |
| probe-04 | 显式重置 Sahara 状态机，上传原包 loader 并收到完成响应；启动日志显示 VIP 等待签名表；直接 configure 被当作错误表输入并 NAK；未发送 GPT 读取 |
| probe-05 | 试图打开上一连接时设备已消失；未发送数据 |
| probe-06 | 新的 ADB → EDL 会话；直接上传原包 loader，发送同包 NV 10010111 签名表，取得 ACK；configure ACK；getsigndata ACK；GPT 读取仅有拒绝日志，随后超时，结束会话 |

probe-06 的 `DigestWrite` 与主机重算的整份签名表 SHA-256 一致。getsigndata 返回 `Mode:W`、设备 ID、当前 Rand、NV 和 `SecureBoot:enable`。旧记录验签失败，DigestRead 是占位串。没有发送 verify 或任意签名替代物，没有修改设备返回值、公钥或 programmer。

configure 同时记录 `Mode= Invalid value` 和 `EnableFlash not found`，尽管最终返回 ACK。这些日志保留为配置接入缺口，不把 ACK 等同于完整厂商流程。为区分该缺口与当前拒读原因，本次还静态检查了**本包** programmer：

- 命令表 `0x149cd120` 将 read 指向 `0x149a297c`。
- read 入口通过 `0x149a88e4` 读取 `0x80269b48` 的授权状态，未满足时走拒绝路径；特殊模式另有分支，本次未选择或尝试。
- EnableFlash 的 setter `0x149a8910` 写入另一个地址 `0x149da640`，不能把它与上述授权状态合并。
- 已找到的授权 setter 调用位于签名接收／验证成功路径 `0x149a14ac–0x149a14b0`。没有改动这些状态。

静态片段见[读取权限证据](../../runs/phase-6-rmx3800-20261115/read-permission-evidence.txt)和[配置字段证据](../../runs/phase-6-rmx3800-20261115/permission-gates.txt)。这些片段支持将当前拒读与授权门槛联系起来；它们不是完整模式或授权流程的实机覆盖。后续仍须对齐配置、取得合法的当前会话签名并分别验证权限和写入。

读取日志不包含最终 NAK，因而本次结果为**读取超时**，不能标作可安全继续的 `ReadRejected`。没有在超时后继续发送设备命令或自动复位。用户随后用硬件按键重启手机。ADB 读回确认系统仍为原来的 16.0.5.740，green、locked、`_b`、NV 10010111 均未改变，`sys.boot_completed=1`。

## 产品改动

新增公共参数 `--signed-digest <file>`，保留原有五个工作流。它是用户显式选择的签名分区摘要表输入，不是在线授权或通用会话签名接口。

1. 在打开设备前，将普通文件读入有界缓冲，拒绝空文件和超过 16 MiB 的输入。
2. 在 Sahara 与启动日志之后、configure 之前发送原始字节。此处不用 XML 写入器，避免给 512 字节整数倍输入追加换行。
3. 等待完整 ACK，检查响应已回到 XML 命令模式；NAK、异常 rawmode、超时或发送失败均停止。没有重试或隐式 reset。
4. 将连接阶段和启动日志通过回调交给 CLI 的 stderr，保留 JSON stdout 只有一个结果对象的契约。错误保留 Sahara、启动、摘要表和配置阶段。
5. XML 预检错误现在保留文件上下文。源数据不足时同时列出节点、LUN、label、源文件、偏移、请求字节及剩余字节。

未加入会话签名生成、厂商账户、绕过流程、短容器转换、super 构建或完整包自动选项。原有严格长度检查保持有效，没有为让原包通过而自动补齐分区尾部。

## 固件输入检查

整包校验和解压完成后，审计常规 rawprogram0–5，共 157 个 program 节点。按当前解析契约有 42 个输入问题，其中 5 个字面路径缺失，37 个普通镜像的声明范围超过允许的末扇区补零范围。

- 缺少字面路径：`super.img`、`oplus`、`sec.dat`、`dynamic_nvbk.bin`、`static_nvbk.bin`。
- `META/super_def.10010111.json` 与设备 NV 相符，但仅有定义和分区素材，尚未构建、验证 super。
- 短镜像包括普通 ELF/MBN、vbmeta 和其他格式。当前没有依据将所有这些项统一补零、裁短或换名。
- 包内其他变体、别名容器、DISK patch 与动态分区布局仍需按对应格式处理。

所以即使补齐设备授权，当前产品也不能把这个 ZIP 直接当作已经支持的整包执行。完整条目见[输入审计](../../runs/phase-6-rmx3800-20261115/rawprogram-input-audit.json)。

使用新构建的 CLI 对完整解压目录中的 rawprogram0–5 执行预检，程序在枚举设备前以 `input_error`／退出码 2 停止，明确指出 rawprogram0 第 6 个节点 `vbmeta_system_a` 请求 65,536 字节，而源文件只有 8,192 字节。没有打开设备。见[产品预检结果](../../runs/phase-6-rmx3800-20261115/product-preflight.json)。

## 验证与证据

本次 macOS 的格式检查、Clippy（警告为错误）、构建及全部 93 个常规离线测试通过。JSON 子进程检查分别覆盖有／无签名表两条路径，并验证进度不污染 stdout。本次未重新运行 Windows／Linux 原生 CI。

新增回归覆盖：摘要表逐字节传输（包含 512 字节边界）、分片输入、NAK／超时／日志后超时／错误 rawmode 停止、空表在 Sahara 前拒绝、初始化错误阶段、CLI 输入先验拒绝、固件错误上下文。

另外，用 probe-06 捕获的原始 Sahara 请求、启动日志、摘要表 ACK 和配置响应，配合同一原始 programmer／摘要表，离线重放到**新增产品初始化函数**。重放通过，发送的表逐字节一致，只有一次 configure，没有 reset。启动日志后的 idle timeout 按实测边界显式插入；这是传输重放，不是新增产品路径已在设备上重新完成验收。见[重放结果](../../runs/phase-6-rmx3800-20261115/product-replay.json)。

- [实机组合、哈希和结果摘要](../../runs/phase-6-rmx3800-20261115/acceptance-summary.json)
- [完整 ZIP 校验](../../runs/phase-6-rmx3800-20261115/firmware-verification.json)
- [probe-06 本地日志](../../runs/phase-6-rmx3800-20261115/probe-06/probe.log)（含设备会话信息）
- [设备恢复后的 ADB 读回](../../runs/phase-6-rmx3800-20261115/adb-after.txt)
- [Clippy](../../runs/phase-6-rmx3800-20261115/clippy.log)、[测试](../../runs/phase-6-rmx3800-20261115/test.log)、[构建](../../runs/phase-6-rmx3800-20261115/build.log)、[格式](../../runs/phase-6-rmx3800-20261115/fmt.log)

同时重新读取 [Anteasy 公共发布列表](https://oplustool.anteasy.com/api/public/releases?page=1&page_size=10)：当前列表中最新可用预览版仍为 4.2026.009，标记 is_latest 的正式版为 v3.2026.028。此查询没有登录、领取业务授权、下载云端 loader 或使用签名服务。结合[已有研究](../research/anteasy-oplustool-4.2026.009-analysis.md)，没有得到一份可以替代当前会话授权的已验证公开材料。没有将第三方工具的型号目录当作授权或刷写证据。

本次未提交或推送。原有未提交研究、计划和 runs 内容保留。

后续[会话授权核查](../research/rmx3800-session-authorization.md)已将本次真实挑战与原包及官方主机端的 OSIG/SHA 输出对齐，并完成原始 RSA 拒绝路径验证。用户另行批准的一次第三方签发请求返回“账户不存在”，没有签名；该后续研究未再次操作手机。OplusTool 正常客户端取得配套云端资产的路径仍未排除。
