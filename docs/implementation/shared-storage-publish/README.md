# 通用存储接口发布与固定依赖验证

## 发布结果

qdlrs 已按用户明确指令提交并推送至 [LanceaKing/qdlrs 的 oplus-edl-phase1 分支](https://github.com/LanceaKing/qdlrs/tree/oplus-edl-phase1)。提交为 [`205c7d4d9f15add22d78c422ba77843ab72c95d4`](https://github.com/LanceaKing/qdlrs/commit/205c7d4d9f15add22d78c422ba77843ab72c95d4)，父提交为 `a80cd1e8cd1cd60634b9c5ce9a4c8c96b727c003`。

提交包含已审查的 11 个源码、测试和文档文件，保留 `Assisted-by: Codex:GPT-6`，未代签 `Signed-off-by`。两轮审查快照留在 qdlrs 本地 `reviews/` 中，没有加入代码提交；qdlrs 已跟踪文件无未提交改动。未向上游仓库提交 PR。

## 产品依赖

Cargo.toml 与 Cargo.lock 均固定上述完整 Git revision，临时 `../qdlrs/qdl` 路径依赖已移除。README、第三方来源说明、依赖记录、测试文档及提取记录已更新。产品仓库保留已有在途改动，本次未提交或推送产品仓库。

通过 `git ls-remote` 核对远端分支指向该提交。Cargo 实际从远端获取源码后，`cargo metadata --locked --offline --format-version 1` 返回 Git 来源与对应 Cargo 缓存路径。11 个提交文件的 SHA-256 均与第二轮审查快照一致。证据见 [dependency-source.json](dependency-source.json)，提交与环境摘要见 [summary.json](summary.json)。不依赖相邻 qdlrs 工作区完成产品构建。

## 重新验证

两边均通过以下检查。qdlrs 检查发生在提交前，检查内容与提交一致；产品检查使用新固定 Git 来源。

| 检查 | qdlrs | 产品 |
| --- | --- | --- |
| 格式检查 | 通过 | 通过 |
| Clippy，workspace／全部目标／全部功能，拒绝警告 | 通过 | 通过 |
| workspace／全部功能测试 | 95 通过 | 135 通过，1 个既有测试忽略 |
| JSON 输出子进程 | 不适用 | 通过 |
| workspace／全部功能构建 | 通过 | 通过 |

Clippy、测试及构建均使用 `--locked --offline`。精确命令和退出码见 [qdl-checks.json](qdl-checks.json) 与 [product-checks.json](product-checks.json)，本目录保留对应日志。全部检查在本机 macOS 完成；未操作设备，未执行新的 Windows／Linux 原生验收，未将历史 CI 结果作为本次提交验证。
