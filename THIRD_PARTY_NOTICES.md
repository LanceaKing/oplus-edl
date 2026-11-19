# 第三方来源与许可证

当前链接的 qdl 固定为 [`9161c0f46cef245f3b5fc5394dba1ec5e8c3f6ac`](https://github.com/LanceaKing/qdlrs/commit/9161c0f46cef245f3b5fc5394dba1ec5e8c3f6ac)，包含通用存储接口和应答等待修复，使用 [BSD-3-Clause](licenses/qdlrs-BSD-3-Clause.txt)。

历史上，产品使用 `a80cd1e8cd1cd60634b9c5ce9a4c8c96b727c003` 接入 Sahara 加载器模式修复，使用 `205c7d4d9f15add22d78c422ba77843ab72c95d4` 接入已发布的通用存储接口。

产品原创代码使用 MIT。以下复用源码保留原 BSD-3-Clause 许可。

- GPT 读取、有界分配、GPT 校验、唯一名称定位及单分区传输位于 fork 的 `qdl/src/storage.rs`，保留 Qualcomm 与 LanceaKing 的版权。产品 `crates/oplus-edl-core/src/storage.rs` 保留目录、LUN 策略和适配层。
- 核心离线测试的 ScriptedIo 及设备构造辅助函数来自 fork 的 `qdl/tests/support/mod.rs`。产品新增了合成 GPT 与测试会话辅助函数。
- `crates/oplus-edl-core/src/programmer.rs` 的 CPIO newc 解码来自锁定的 `qdl/src/lib.rs`，改为读取核心已持有的加载器句柄，保留 Qualcomm 的版权和 BSD-3-Clause 许可。

早期存储与测试复用的历史基线来自 [LanceaKing/qdlrs，7e0a31a5be2ab5d01b27bd743ee67a445ad0b84d](https://github.com/LanceaKing/qdlrs/tree/7e0a31a5be2ab5d01b27bd743ee67a445ad0b84d)。BSD 条款及上游版权文本保存在 [licenses/qdlrs-BSD-3-Clause.txt](licenses/qdlrs-BSD-3-Clause.txt)。

`Cargo.lock` 记录实际解析版本。`clap`、`serde`、`serde_json`、`anyhow`、`libc` 和 `windows-sys` 按各 crate 声明的 MIT 或 Apache-2.0 许可使用。`xmltree` 0.11.0 使用 MIT。XML 与稀疏解析为产品原创实现，行为参照设计指定的 qdlrs 与 qdl 版本。

## 发布包

- USB 依赖 `nusb` 0.2.9，使用 Apache-2.0 或 MIT。云端依赖 `wreq` 0.16.3 和 `btls` 0.5.6，均使用 Apache-2.0。`btls-sys` 0.5.6 使用 MIT，包内另附其 BoringSSL 源码自带的许可文本。
- eframe、egui 和 egui_extras 0.37.0 由 [egui 项目](https://github.com/emilk/egui/tree/0.37.0)提供，可按 MIT 或 Apache-2.0 使用。包内附 [MIT 条款](licenses/egui-MIT.txt)。
- [rfd 0.18.0](https://docs.rs/rfd/0.18.0/rfd/) 使用 [MIT 条款](licenses/rfd-MIT.txt)。
- 包内嵌未修改的 Noto Sans CJK SC Regular 2.004。字体版权 © 2014–2021 Adobe，使用 SIL Open Font License 1.1。包内的 `licenses/NotoSansCJK-OFL.txt` 附完整文本。

每个平台的包内 `licenses/dependencies.json` 列出该目标的正常和构建依赖版本、许可声明及随包文件。`licenses/cargo/` 收录锁定源码自带的文本，并按已锁定版本补充上游声明。`dispatch` 0.2.0 的条目使用上游 Cargo 清单所声明的 MIT 许可元数据。
