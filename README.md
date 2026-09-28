# Pomelo OS · 柚儿园

一个只属于我自己的 macOS 本地个人系统，用来管理生活数据和个人兴趣。所有数据都保存在本机，不联网同步。

## 模块

| 模块 | 用途 | 状态 |
|---|---|---|
| 摸鱼读读 | 书单 | ✅ 可以用了 |
| 好奇看看 | 影单（电影、剧集、动漫） | ✅ 可以用了 |
| 随意画画 | 绘画课程进度 | v0.1 计划中 |
| 出门走走 | 旅行行李清单 | v0.1 计划中 |

以后还会加入：Todo、个人财务、Dashboard、全局搜索、数据统计、AI 助手。

## 当前进度

阶段 3（好奇看看）已完成，下一步是阶段 4：随意画画。详见 [开发路线](docs/roadmap.md)。

## 文档

| 文档 | 内容 |
|---|---|
| [开发路线](docs/roadmap.md) | 进度、各阶段要做什么、怎样算完成 |
| [技术架构](docs/architecture.md) | 技术选型、数据存放、备份与升级 |
| [数据结构](docs/schema.md) | 每张表的字段，以及 Excel 如何导入 |
| [视觉规范](docs/design.md) | 颜色、字体、布局、文案风格 |
| [CLAUDE.md](CLAUDE.md) | 给 AI 编程助手的工作规则 |

## 目录

| 位置 | 内容 |
|---|---|
| `docs/` | 项目文档 |
| `src/` | 界面（React + TypeScript），按模块分文件夹 |
| `src-tauri/` | 数据层和应用外壳（Rust），`migrations/` 里是数据库升级脚本 |
| `scripts/` | 辅助脚本，比如 Excel 转换 |
| `prototype/` | Excel 原型，只在本机，不进 Git |

## 数据在哪里

- 个人数据：`~/Library/Application Support/Pomelo OS/`（第一次打开正式版时生成）
- 开发时的数据：`~/Library/Application Support/Pomelo OS Dev/`，和真实数据分开

## 运行

- **日常使用**：运行 `pnpm app:install`，把最新版装进「应用程序」，之后像普通 App 一样打开。
- **开发**：运行 `pnpm app:dev`，使用测试数据，不影响日常使用的数据。
