# Pomelo OS · 柚儿园

只属于我自己的 macOS 本地个人系统，用来管理生活数据和个人兴趣。所有数据都保存在本机，不联网同步。

## 模块

| 模块 | 用途 | 状态 |
|---|---|---|
| 摸鱼读读 | 书单 | ✅ |
| 好奇看看 | 影单（电影、剧集、动漫） | ✅ |
| 随意画画 | 绘画课程进度 | ✅ |
| 出门走走 | 行李清单（场景模板 + 每次出行） | ✅ |

以后还会加入：Todo、个人财务、Dashboard、全局搜索、数据统计、AI 助手。

## 当前进度

v0.1 的四个模块都能用了（阶段 0～5）。下一步是**阶段 6：数据安全，正式启用**。详见 [开发路线](docs/roadmap.md)。

## 怎么用

- **安装或更新**：运行 `pnpm app:install`，最新版会装进「应用程序」。只用「应用程序」里的这一个。
- **从 Excel 导入**：运行 `pnpm import:xlsx` 生成导入文件，再在 App 的「设置 → 导入」里选择 `prototype/pomelo-import.json`。
- **开发**：运行 `pnpm app:dev`，用的是测试数据，不影响日常数据。

## 文档

| 文档 | 内容 |
|---|---|
| [开发路线](docs/roadmap.md) | 做到哪了、下一步做什么 |
| [技术架构](docs/architecture.md) | 技术选型、数据存放、备份与升级 |
| [数据结构](docs/schema.md) | 每张表的字段、Excel 怎么导入 |
| [视觉规范](docs/design.md) | 颜色、字体、布局、文案 |
| [CLAUDE.md](CLAUDE.md) | 给 AI 编程助手的工作规则和代码地图 |

## 数据在哪里

- 真实数据：`~/Library/Application Support/Pomelo OS/`
- 开发用的测试数据：`~/Library/Application Support/Pomelo OS Dev/`
- Excel 原型：`prototype/`（只在本机，不进 Git）
