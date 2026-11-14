# 第一阶段 fork 分支推送记录

日期：2026-11-13。按用户明确指令，将已独立复审的两个本地提交推送到 [LanceaKing/qdlrs 的新分支 `oplus-edl-phase1`](https://github.com/LanceaKing/qdlrs/tree/oplus-edl-phase1)：

- [`bd136c484ba4c21e09c14230d10cd0c8956a2fcf`](https://github.com/LanceaKing/qdlrs/commit/bd136c484ba4c21e09c14230d10cd0c8956a2fcf)：第一阶段协议修复、离线回归和 3 个 P2 修复。
- [`7e0a31a5be2ab5d01b27bd743ee67a445ad0b84d`](https://github.com/LanceaKing/qdlrs/commit/7e0a31a5be2ab5d01b27bd743ee67a445ad0b84d)：welcome 结束标记衔接修复及对应回归。

推送前确认远端不存在同名分支、本地工作区干净、提交链只包含上述两个修正提交，且源码与已复审快照一致。推送使用“目标引用必须不存在”的 lease 条件，只创建该分支。命令结果明确报告 `[new branch]`，见[推送日志](push.log)。没有修改提交、增加签署行、推送其他分支或创建 PR。

推送后进行了两项远端确认：

1. `git ls-remote` 返回目标分支头为 `7e0a31a5be2ab5d01b27bd743ee67a445ad0b84d`，见[引用记录](ls-remote.log)。
2. 从空临时 bare 仓库重新拉取该分支，核对 `7e0a31a → bd136c4 → 75fe2f7` 的父子关系，并确认两个修正提交的文件树与本地一致，见[拉取日志](fresh-fetch.log)。临时仓库已自动删除。

完整远端地址、分支、提交、父提交和 tree 值见[核验记录](remote-verification.json)。[固定依赖记录](../../dependencies/qdlrs.md)已更新为完整 revision `7e0a31a5be2ab5d01b27bd743ee67a445ad0b84d`，后续产品集成使用此提交而不是浮动分支名。

本次没有源码变更，沿用同一已复审快照的 81 个测试及四项检查结果；远端 Git 对象核验不等于远端 CI 验证。本地分支跟踪 `origin/oplus-edl-phase1`，工作区干净。未枚举、打开或操作设备。
