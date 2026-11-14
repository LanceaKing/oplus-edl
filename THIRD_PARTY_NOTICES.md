# 第三方来源与许可证

2026-11-14 更新：产品实际链接的 qdl 固定为 `a80cd1e8cd1cd60634b9c5ce9a4c8c96b727c003`，已发布并包含 Sahara 加载器模式修复，保留 BSD-3-Clause。下文旧 revision 是源码复用的历史基线。产品新增 libc 和 windows-sys 依赖，均保留其 MIT／Apache-2.0 许可。许可元数据见[修复记录附件](docs/implementation/phase-2-repair/verification/dependency-licenses.json)，新 Git 来源见[发布记录](docs/implementation/phase-2-publish/dependency-source.json)。

产品原创代码使用 MIT。下列复用文件保留 BSD-3-Clause 标记，不因产品许可改变其许可：

- `src/storage.rs` 的 GPT 读取方式改编自 qdlrs `cli/src/util.rs`，保留 Qualcomm 版权说明。新增有界读取、GPT 校验、唯一名称定位和单分区传输辅助函数。
- `tests/support/mod.rs` 的 ScriptedIo 及设备构造辅助函数来自 fork 的 `qdl/tests/support/mod.rs`；产品新增合成 GPT 与测试会话辅助函数。

上述源码和链接的 qdl 库固定来自 [LanceaKing/qdlrs，7e0a31a5be2ab5d01b27bd743ee67a445ad0b84d](https://github.com/LanceaKing/qdlrs/tree/7e0a31a5be2ab5d01b27bd743ee67a445ad0b84d)。BSD 条款及上游版权文本保存在 [licenses/qdlrs-BSD-3-Clause.txt](licenses/qdlrs-BSD-3-Clause.txt)。上游基线与修正说明见[依赖记录](docs/dependencies/qdlrs.md)。未复制 bkerler/edl 的 GPL 源码。

Cargo.lock 保存本次实际解析版本。直接依赖 clap、serde、serde_json、anyhow、gptman 使用其各自 MIT／Apache-2.0 许可；qdl 使用 BSD-3-Clause。测试依赖 tempfile、bincode 使用各自 MIT／Apache-2.0 许可。完整直接与传递依赖许可元数据见第二阶段的 `verification/dependency-licenses.json`。此元数据不是许可证全文集合；之后制作分发包时须随包保留适用的第三方许可文本。本阶段不生成发布包，也不分发加载器或固件。

第三阶段新增直接依赖 xmltree 0.11.0（MIT，原已由 qdl 间接依赖）。XML 与稀疏解析为产品原创实现，行为参照设计固定的 qdlrs 与 qdl；未复制 qdl 稀疏解析 C 源码或 EDL Python 源码。
