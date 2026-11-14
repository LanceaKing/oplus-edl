# 第二阶段 qdlrs 发布与产品固定依赖

日期：2026-11-14。按用户明确指令，将通过独立复审的 qdlrs 修复提交并推送，随后更新产品依赖。未提交产品代码、未操作设备；未修改产品或 qdlrs 的实现逻辑。

## 提交与远端核验

- Fork：[LanceaKing/qdlrs](https://github.com/LanceaKing/qdlrs)。
- 分支：`oplus-edl-phase1`，普通 fast-forward 推送，没有强制覆盖或推送其他分支。
- 新提交：[`a80cd1e8cd1cd60634b9c5ce9a4c8c96b727c003`](https://github.com/LanceaKing/qdlrs/commit/a80cd1e8cd1cd60634b9c5ce9a4c8c96b727c003)。
- 父提交：`7e0a31a5be2ab5d01b27bd743ee67a445ad0b84d`。
- 仅包含复审的四个文件：AGENTS.md、README.md、qdl/src/sahara.rs、qdl/tests/sahara_transfer.rs。提交前逐文件 SHA-256 和整个 diff SHA-256 均与已复审归档一致。
- 按用户明确授权提交、推送，提交消息保留 `Assisted-by`，未代加 `Signed-off-by`。

先核对远端仍在父提交，再提交并普通推送。推送后通过 `ls-remote` 检查分支头，并在空临时 bare 仓库从远端重新拉取，确认提交号和文件树与本地一致。qdlrs 工作区干净。

证据：[提交对象](commit.json)、[推送日志](push.log)、[远端引用](remote-after.log)、[重新拉取日志](fresh-fetch.log)、[远端对象核验](remote-verification.json)。

## 产品依赖

Cargo.toml 的 rev 已改为上述完整提交号。删除了 `.cargo/config.toml` 的本地 path override，并清理仅用于该临时配置的 gitignore 条目；移除的配置保存在本目录，仅作为历史附件，不参与 Cargo 配置加载。

Cargo.lock 已解析为新 Git 来源。`cargo metadata --locked` 确认 qdl 的 source 为 `git+https://github.com/LanceaKing/qdlrs?rev=a80cd1e8cd1cd60634b9c5ce9a4c8c96b727c003#a80cd1e8cd1cd60634b9c5ce9a4c8c96b727c003`，manifest 位于 Cargo Git checkout，不再指向本地 qdlrs 工作区。见[解析来源](dependency-source.json)。

迁移时 `cargo update -p qdl` 返回 package ID 不匹配；随后 Cargo 检查及 metadata 已确认新依赖和锁文件解析成功。没有把失败的 update 命令记为成功，也没有恢复本地覆盖。

原修复和复审记录描述的是当时的本地覆盖状态，保留为历史证据；当前状态以本记录和依赖说明为准。产品文件保持未提交。

## 检查

| 范围 | 格式 | Clippy | 离线测试 | 构建 |
| --- | --- | --- | --- | --- |
| qdlrs，提交前同一源码 | [通过](qdlrs-fmt.log) | [通过](qdlrs-clippy.log) | [82 个通过](qdlrs-test.log) | [通过](qdlrs-build.log) |
| 产品，新固定 Git 依赖 | [通过](product-fmt.log) | [通过](product-clippy.log) | [37 个通过](product-test.log) | [通过](product-build.log) |

另有 [Windows x86_64 GNU 交叉检查](product-windows-check.log)通过。完整命令与返回码见 [qdlrs-checks.json](qdlrs-checks.json)及 [product-checks.json](product-checks.json)。

本机为 macOS arm64；没有 Windows 程序运行、Linux 实际构建／运行、exFAT 实跑或设备验证。推送成功与远端对象核验不代表远端 CI 已通过，本次没有宣称远端 CI 结果。
