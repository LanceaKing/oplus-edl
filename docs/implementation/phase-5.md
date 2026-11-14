# 第五阶段实施记录

日期：2026-11-14。**第五阶段已开始，尚未达到三平台原生检查全部通过的完成条件。** 本次完成 macOS 原生检查、Windows GNU 和 Linux musl 交叉编译，以及三平台 CI 配置。Windows／Linux 原生运行环境尚未提供，CI 未运行，不把交叉编译当作原生测试。

未提交、未推送、未枚举或操作设备。没有修改产品协议实现、qdlrs fork、固定 revision 或 Cargo.lock。依赖仍为 `a80cd1e8cd1cd60634b9c5ce9a4c8c96b727c003`。开始时产品文件已全部处于未跟踪状态，本次保留该状态。

## 检查结果

| 环境 | 格式检查 | Clippy | 构建 | 离线测试执行 |
| --- | --- | --- | --- | --- |
| macOS arm64 原生 | 通过 | 全目标／全部功能通过 | 通过 | 88 个测试及新增 JSON 协议子进程检查通过 |
| Windows x86_64 GNU，由 macOS 交叉编译 | 使用 macOS 的格式检查，不算 Windows 原生检查 | 全目标／全部功能通过 | 已链接 EXE | `--no-run` 编译通过；未执行 |
| Linux arm64 musl，由 macOS 交叉编译 | 使用 macOS 的格式检查，不算 Linux 原生检查 | 全目标／全部功能通过 | 已链接 ELF | `--no-run` 编译通过；未执行 |
| Linux arm64 GNU，由 macOS 跨目标检查 | 同上 | 全目标／全部功能通过，`cargo check` 也通过 | 失败：本机缺少 Linux GNU 链接工具链 | 未执行 |
| Windows／Linux 原生 | 未执行 | 未执行 | 未执行 | 未执行 |

本机 Rust／Cargo 为 1.99.0。macOS 和 Windows GNU、Linux musl 最终检查均使用已有缓存、`--locked --offline`，没有重新解析依赖。格式检查不解析依赖。Linux GNU 初次检查使用 `--locked`，其失败日志保留；macOS 的系统链接器无法处理 Linux 参数，不能据此判断产品的 Linux 原生构建失败。musl 成功不替代 GNU 或原生验证。

原生日志：[格式](phase-5-verification/macos-fmt.log)、[Clippy](phase-5-verification/macos-clippy.log)、[构建](phase-5-verification/macos-build.log)、[测试](phase-5-verification/macos-test.log)。

交叉编译日志：[Windows Clippy](phase-5-verification/windows-cross-clippy.log)、[Windows 构建](phase-5-verification/windows-cross-build.log)、[Windows 测试编译](phase-5-verification/windows-cross-test-no-run.log)、[Linux musl Clippy](phase-5-verification/linux-musl-cross-clippy.log)、[Linux musl 构建](phase-5-verification/linux-musl-cross-build.log)、[Linux musl 测试编译](phase-5-verification/linux-musl-cross-test-no-run.log)、[Linux GNU 检查](phase-5-verification/linux-cross-check.log)、[Linux GNU Clippy](phase-5-verification/linux-cross-clippy.log)、[Linux GNU 链接失败](phase-5-verification/linux-cross-build.log)。

各环境的 `*-summary.json` 保存完整命令和退出码。[环境记录](phase-5-verification/environment.log)保存主机和工具链信息；[源码摘要](phase-5-verification/source-sha256.json)标识本次检查使用的源文件、Cargo 文件和 CI 配置。

## 补齐的检查

- 新增 [CI 工作流](../../.github/workflows/ci.yml)，分别在 Windows、macOS、Ubuntu runner 上执行原生格式检查、Clippy、构建和离线测试。先用 `cargo fetch --locked` 获取依赖，随后全部使用锁定的离线依赖。工作流只运行 Cargo 检查，不运行设备命令。YAML 已本地解析检查，未触发 GitHub Actions，不能将配置存在记录为 CI 通过。
- 新增 [JSON stdout 检查](../../tests/json_stdout.rs)，以独立子进程捕获真实 stdout。使用产品配置和内存传输，执行 Sahara 加载、Firehose welcome／configure、短镜像写入、补零和回读，在协议响应之间注入日志，并检查输出恰好为一个 JSON 对象，且包含 acknowledged／matched。对象释放也在输出捕获范围内，记录中不得出现 reset。
- 此检查设置 `harness = false`，避免测试框架文字混入被检查的输出；Cargo 的普通测试计数仍是 88，新增检查由单独的 `JSON stdout protocol subprocess: passed` 行证明通过。Windows 的 Unix FIFO 两个测试不会编译进入测试集，原生测试数量应按实际日志记录，不要求与 macOS 数字相同。

现有检查未发现需要修改产品代码的缺陷。修补的是 CI 缺失和此前 JSON 测试仅覆盖设备访问前路径的验证缺口。新测试初次编译时修正了测试代码中单个 Mapping 与 Vec 的类型不匹配，最终四项检查通过。

## 复现与剩余验收

三平台原生环境均运行：

```sh
cargo fetch --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings
cargo build --workspace --all-features --locked --offline
cargo test --workspace --all-features --locked --offline
```

Windows 交叉编译使用 `x86_64-pc-windows-gnu`，并设置 `CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc`。Linux musl 使用 `aarch64-unknown-linux-musl`，将当前 Rust sysroot 的 `lib/rustlib/aarch64-apple-darwin/bin` 加入 PATH，设置 `CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld`。给上述 Clippy／build／test 加上 `--target <目标>`，test 再加 `--no-run`；这些命令不执行目标程序。本次新增安装了 Linux GNU 和 musl 的 Rust 标准库目标，没有安装驱动或改动设备权限。

待提供 Windows、Linux 原生主机，或后续在允许提交／推送后运行已配置的 CI，再追加各主机架构、工具链、四项检查日志和实际测试数量。当前不宣称第五阶段完成，不验证真实 USB／串口、Windows MSVC 运行行为、Linux GNU 运行行为或文件系统运行差异。第六阶段设备验证仍须另行授权。
