# GitHub 配置

[ci.yml](workflows/ci.yml) 在 push、pull_request 和手动触发时执行三平台原生检查。同一分支的新运行会取消本工作流的旧运行，矩阵内一个平台失败不会取消其他平台。

每个平台安装 wreq / btls-sys 所需的原生构建依赖，使用 Rust stable、rustfmt 和 Clippy。先在线获取 Cargo.lock 锁定的依赖，再用 --locked --offline 执行 Clippy、构建和测试。下载缓存按系统、架构和 Cargo.lock 区分，不缓存本地 runs、固件或设备材料。

默认测试包含脚本化设备协议和本地回环 HTTP fixture。真实云端探测仍保持 ignored，不使用 --include-ignored，也不执行手机操作。独立 JSON stdout 测试由 Cargo.toml 的 test target 随 cargo test 执行。工作流只申请 contents: read，checkout 不持久化凭据。

原生依赖依据：[wreq 构建说明](https://github.com/0x676e67/wreq#building)及[三平台编译示例](https://github.com/0x676e67/wreq/blob/main/.github/compilation-guide/build.yml)。本地检查与实机证据边界见[测试说明](../docs/testing.md)，项目文档见[导航](../docs/README.md)。

修改工作流后，应校验 YAML 与 Actions 表达式。三平台最终结果须以修改后的 GitHub Actions 运行记录为准；历史阶段五通过记录不证明当前配置已运行通过。
