# 开发与打包

| crate | 职责 |
| --- | --- |
| `oplus-edl-core` | `CoreService`、设备会话、操作预检、执行、停止和结果 |
| `oplus-edl-cli` | `oplus-edl` 的命令语法、信号和文本或 JSONL 输出 |
| `oplus-edl-desktop` | `oplus-edl-desktop` 的工作区、文件选择、确认和状态显示 |

在对应架构的原生主机准备符合锁定依赖版本要求的 Rust stable，以及 `rustfmt`、`clippy` 和 Python 3.9 以上。生产云端特性通过 `btls-sys` 构建 BoringSSL，需要 C/C++ 工具链、CMake、Perl、`pkg-config` 和 libclang。按平台使用以下工具。

| 构建主机 | 工具 |
| --- | --- |
| Windows x86_64 | MSVC C++ 构建工具和 Windows SDK、PowerShell 7、CMake、Perl、pkg-config、LLVM 和 NASM。确保 `python` 和工具可从 `PATH` 调用。 |
| macOS arm64 | Xcode 命令行工具，另准备 CMake、pkg-config 和 LLVM。需要时将 `LIBCLANG_PATH` 指向 LLVM 的 `lib`。 |
| Ubuntu 24.04 x86_64 | `build-essential`、`cmake`、`perl`、`pkg-config`、`libclang-dev` 和 GNU tar。 |

CI 使用 `windows-2025`、`macos-15` arm64 和 `ubuntu-24.04`。macOS 14 托管镜像已退役，打包仍设置 `MACOSX_DEPLOYMENT_TARGET=14.0`。[CI 配置](../.github/workflows/ci.yml) · [GitHub runner 镜像](https://github.com/actions/runner-images/blob/main/README.md) · [退役公告](https://github.com/actions/runner-images/issues/13518)

先获取锁定依赖。缓存准备后，可离线运行 [工作区检查](testing.md)或从仓库根目录启动桌面。

```sh
cargo fetch --locked
cargo run -p oplus-edl-desktop --locked --offline
```

## 生成发布包

在 macOS arm64 或 Ubuntu x86_64 上运行 `sh scripts/package-desktop.sh`，在 Windows x86_64 的 PowerShell 7 中运行 `./scripts/package-desktop.ps1`。脚本会锁定版本和原生目标，以 `--locked --offline --release --bins --no-default-features` 构建。显式启用以下特性。

```text
oplus-edl-cli/usb,oplus-edl-cli/serial,oplus-edl-cli/cloud,oplus-edl-desktop/usb,oplus-edl-desktop/serial,oplus-edl-desktop/cloud
```

两个前端向核心转发这三项特性。仅用于测试的 `test-support` 由回归测试启用。打包脚本检查生产依赖图、归档布局、二进制格式和逐文件哈希，成功后在 `dist/` 生成 [发布归档](distribution.md)并打印 SHA-256。可在任一装有 Python 3.9 以上版本的主机上重复归档核验。

```sh
python3 scripts/desktop-release.py verify aarch64-apple-darwin dist/oplus-edl-0.1.0-macos-arm64.zip
```

## 独立使用核心

在自己的 Rust 项目中通过路径引用核心 crate。核心默认特性为空。按需启用 `usb`、`serial` 或 `cloud`。

```toml
[dependencies]
oplus-edl-core = { path = "/path/to/oplus-edl/crates/oplus-edl-core", default-features = false }
```

用 `CoreService::new` 和 `HostServices::default().with_backend(...)` 注入枚举和传输打开函数。启用原生后端时可用 `HostServices::native()` 提供对应实现。`submit` 返回 `JobHandle`。用 `poll_event` 读取事件，用 `try_take_result` 取出终态并释放任务槽，或用 `request_stop` 请求协作停止。写入任务通过 `prepared_summary` 和 `decide` 处理本次确认。可参考 [public_api 回归](../crates/oplus-edl-core/tests/public_api.rs)中的独立调用方式。
