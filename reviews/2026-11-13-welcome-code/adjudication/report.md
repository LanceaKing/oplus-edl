# Welcome/XML whitespace 边界独立裁定

裁定：**现有证据不足以成立新的 P1/P2 缺陷，不应把本例列为“新代码吞掉请求范围内的原始数据”。** 已证实的变化是：读取完请求的 N 字节后，最终响应也接受 XML 声明前的 framing whitespace。按本补丁明确采用的 XML 前导空白规则，这是 framing 接受范围扩展。若要求严格保留最终 ACK 的旧接受集合，则存在范围/契约问题；但给定已批准设计没有规定这一严格规则。发送端究竟只发送了 7 个存储字节，仍属于此字节流本身无法消除的契约歧义。

本裁定只处理指定争议，不替代完整代码评审。没有已证明的、需要以 P1/P2 修复的新增缺陷。

## 可以从证据直接得出的结论

测试请求 2 个、每个 4 字节的扇区，共 8 字节。`firehose_read_storage` 从初始 ACK 的 `</data>` 后开始，逐次读取且限制每次读取长度，直到 `bytes_left == 0`，然后才调用最终 XML 响应解析。它没有在这 8 字节内部执行空白跳过。见 [读取长度与循环](/Users/Apple/Documents/oplus-edl/reviews/2026-11-13-welcome-code/source/qdl/src/lib.rs:695)、[输出和最终响应切换](/Users/Apple/Documents/oplus-edl/reviews/2026-11-13-welcome-code/source/qdl/src/lib.rs:730)。

指定输入在初始 ACK 之后是 `31 32 33 34 35 36 37 0d 0a`，然后才是声明和最终 ACK。两个版本均输出前 8 字节 `31 32 33 34 35 36 37 0d`。新分支消耗的是第 9 字节 `0a`，不是请求范围内的第 8 字节 `0d`。原测试的名称和断言把 CR 定义为 framing，但脚本仅编码字节，没有编码独立的数据结束标志。[原探针](/Users/Apple/Documents/oplus-edl/reviews/2026-11-13-welcome-code/coordinator/probe-source/qdl/tests/review_short_raw.rs:6)、[脚本 Read/BufRead 的共同位置](/Users/Apple/Documents/oplus-edl/reviews/2026-11-13-welcome-code/source/qdl/tests/support/mod.rs:62)。

旧版失败原因是剩余 LF 进入 XML 文档缓冲后，XML 声明不再位于文档开头；不是短数据计数检查。新版先跳过 LF，再读取完整声明和 ACK。[原 current 日志](/Users/Apple/Documents/oplus-edl/reviews/2026-11-13-welcome-code/coordinator/current-short-raw.log)、[原 baseline 日志](/Users/Apple/Documents/oplus-edl/reviews/2026-11-13-welcome-code/coordinator/baseline-short-raw.log)、[修改后的分支](/Users/Apple/Documents/oplus-edl/reviews/2026-11-13-welcome-code/source/qdl/src/lib.rs:246)。

## 独立离线对照

在本目录内建立两个副本。`probe-current` 保留快照；`probe-baseline` 只把条件恢复为 `if welcome && pending.is_empty()`。这是本争议的行为等价基线，未声称完整恢复原提交的文档和测试。两个副本使用同一探针，每例覆盖完整合并输入、每一个两段分割点和逐字节输入；每个版本共 670 种分片。成功例还检查后续 NAK 未被吞掉，并检查仅发送一次 read 命令且没有 reset。

| 初始 ACK 后、最终 ACK 前的输入 | 最终 ACK 有声明 | Current | Baseline | 两版输出 |
| --- | --- | --- | --- | --- |
| `1234567\r\n` | 是 | 成功 | XML 错误 | `1234567\r` |
| `1234567\r\n` | 否 | 成功 | 成功 | `1234567\r` |
| `1234567\r` | 是 | 成功 | 成功 | `1234567\r` |
| `1234567` | 是 | 错误 | 错误 | `1234567<`，随后解析失败 |
| 8 个 `SP/TAB/CR/LF` 字节 | 是 | 成功 | 成功 | 完整保留这 8 字节 |

执行命令分别是 `cargo test --offline -p qdl --test adjudication_boundary -- --nocapture` 和前置 `ADJ_BASELINE=1` 的相同命令，两者均退出 0。[探针](/Users/Apple/Documents/oplus-edl/reviews/2026-11-13-welcome-code/adjudication/probe-current/qdl/tests/adjudication_boundary.rs)、[current 结果](/Users/Apple/Documents/oplus-edl/reviews/2026-11-13-welcome-code/adjudication/current-matrix.log)、[baseline 结果](/Users/Apple/Documents/oplus-edl/reviews/2026-11-13-welcome-code/adjudication/baseline-matrix.log)。

这些结果没有证明发送端永远不会短读；它们证明旧版也没有从该字节流中可靠区分“7 字节数据 + CRLF”与“8 字节数据（以 CR 结尾）+ LF”。移除 XML 声明就消失的失败，不能直接当成稳定的短读检测契约。

## 设计义务和最小范围

批准设计要求完整传输、完整输出和最终 ACK，要求 EOF、硬错误或零进展不得报告完整传输。[设计第 184 行](/Users/Apple/Documents/oplus-edl/reviews/2026-11-13-welcome-code/design.md:184)、[设计第 350 行](/Users/Apple/Documents/oplus-edl/reviews/2026-11-13-welcome-code/design.md:350)。本例存在 8 个可读取字节、输出完整、没有 EOF/硬错误/零进展，并存在完整最终 ACK。这些义务本身没有规定 CRLF 是独立 raw 结束标记，也没有禁止最终响应前存在 framing whitespace。因此不能仅凭“预期原始数据应为 7 字节”的外部标签推出违反这些义务。

补丁的范围确实大于 welcome 交接：所有 `firehose_response` 调用都以 `welcome=false` 使用该解析器，因此移除 `welcome` 条件也改变最终 read ACK 的接受集合。[调用点](/Users/Apple/Documents/oplus-edl/reviews/2026-11-13-welcome-code/source/qdl/src/lib.rs:302)。这不是隐藏影响：新增测试明确在 8 字节 raw 之后放入 CRLF 和带声明 ACK，[测试](/Users/Apple/Documents/oplus-edl/reviews/2026-11-13-welcome-code/source/qdl/tests/firehose_io.rs:476)；README 和 AGENTS 也说明所有 XML 读取接受前导 framing whitespace，[README](/Users/Apple/Documents/oplus-edl/reviews/2026-11-13-welcome-code/source/README.md:32)。这些新增文字证明实现的行为意图，不能单独证明外部协议授权。

如果本次授权把“仅修 welcome”定义为**最终 ACK 的接受集合必须逐项不变**，可以指出这一具体范围差异，要求把状态限定到 welcome 交接。但“任务起因是 welcome”或“采用最小修改”并不自动建立这种字节级不变约束；也不能把范围差异升级为已经证实的数据损失。给定设计要求只做必要修正、避免无关重构，但没有规定该最终响应语法。[设计边界](/Users/Apple/Documents/oplus-edl/reviews/2026-11-13-welcome-code/design.md:156)。

## 可推翻本裁定的证据和边界

满足下列任一项，才足以把争议推进为具体缺陷：

- 提供适用于此传输/API 的明确规则：第 N 字节之后必须立即开始 XML，任何额外空白都表示长度错误；或者提供能独立区分 raw 结束与 XML framing 的通道信息。此时可以提出明确修复：最终 raw 响应采用严格前导规则，把宽松规则限定到已知 welcome 交接。
- 提供在已声明、无歧义的数据边界内，新版跳过/改写请求 N 字节中的任一字节而旧版不跳过的复现。当前例不符合：两版输出逐字节一致。
- 提供已确认的更窄授权，明确禁止对其他 XML 响应扩展接受集合。此时是具体的范围违约，而不必包装成数据损坏。

“最终 ACK 紧接 N 字节、不允许任何 framing 空白”与“最终 ACK 前允许空白”两种规则不能同时用于解释同一输入并要求解析器推断发送端意图。在当前 `BufRead` 字节流抽象下，两种意图没有可观察差异。

限制：本次只读指定快照、设计、补丁和获准的 coordinator 探针/日志，没有读取其他当前评审报告。只在 `adjudication/` 写入副本、探针、构建结果和本报告；未编辑生产文件或不可变快照，未执行应用 CLI、提交、推送或硬件枚举/打开。测试是脚本化离线测试，没有设备协议合规性或真实发送端行为证据。
