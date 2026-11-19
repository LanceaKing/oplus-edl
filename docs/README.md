# 项目文档

| 内容 | 入口 |
| --- | --- |
| 桌面 GUI 与共用核心设计（已定稿） | [中文](designs/oplus-edl-desktop.zh-CN.md) · [English](designs/oplus-edl-desktop.md) · [检查点进度](implementation/desktop-migration.md) |
| 桌面用法 | [桌面工作区](desktop.md) |
| CLI 命令与输出 | [CLI 用法](cli.md) |
| 发布包、启动与访问权限 | [发布指南](distribution.md) |
| 源码构建、生产打包与独立核心 | [开发指南](development.md) |
| 初版 CLI 设计 | [Oplus EDL 设计](designs/oplus-edl.md) |
| 实施与验收进度 | [实施索引](implementation/README.md) |
| 工具与协议研究 | [研究索引](research/README.md) |
| 测试方法与边界 | [测试说明](testing.md) |
| 协议库依赖 | [qdlrs](dependencies/qdlrs.md) |
| 历史简报与设计草稿 | [设计归档](designs/archive/README.md) |
| 评审与修复 | [评审索引](../reviews/README.md) |
| 本地实验与实机证据 | [运行记录](../runs/README.md) |

项目根目录保留 README、Cargo 配置和锁文件、许可证及第三方声明。设计与研究文档放在 docs，评审报告放在 reviews，本地输入与实验产物放在 runs。Cargo 构建缓存继续使用标准 target 目录。
