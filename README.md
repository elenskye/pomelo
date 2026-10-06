# Pomelo OS · 柚儿园

只属于我自己的 macOS 本地个人系统，用来管理生活数据和个人兴趣。所有数据都保存在本机，不联网同步。

## 模块

| 模块 | 用途 | 状态 |
|---|---|---|
| 一步一步 | 待办：分组、目标、任务；重复、打卡、进度条；月历（参考「目标地图」） | v0.2 待验收 |
| 摸鱼读读 | 书单 | ✅ |
| 好奇看看 | 影单（电影、剧集、动漫） | ✅ |
| 随意画画 | 绘画课程进度 | ✅ |
| 出门走走 | 行李清单（场景模板 + 每次出行） | ✅ |
| 钱来钱往 | 记账：收支、固定支出、账户和总资产、报表和预算 | v0.2 待验收 |
| 投石问路 | 求职投递：表格、跟进提醒、统计，笔试面试自动进待办 | v0.2 待验收 |
| 念念不忘 | 笔记与灵感（参考 Flomo）：随手记、多级标签、热力图、互相引用 | v0.2 待验收 |
| 芝麻开门 | 密码：主密码或 Touch ID 才能打开，加密保存，闲置 5 分钟自动锁 | v0.2 待验收 |

以后还会加入：全局搜索、首页、通用清单与进度、本地 AI 等。

## 当前进度

v0.1 已完成，Pomelo 是唯一的数据来源。v0.2 开发中：待办、记账「钱来钱往」、求职「投石问路」、笔记「念念不忘」、密码「芝麻开门」都做完了，正式版已更新，等验收；接下来是全局搜索和侧栏。详见 [需求](docs/requirements.md) 和 [开发路线](docs/roadmap.md)。

## 怎么用

- **安装或更新**：运行 `pnpm app:install`，最新版会装进「应用程序」。只用「应用程序」里的这一个。
- **从 Excel 导入**：运行 `pnpm import:xlsx` 生成导入文件，再在 App 的「设置 → 导入」里选择 `prototype/pomelo-import.json`。
- **导入 Markdown 待办**：运行 `pnpm import:todo`（默认读「下载」里的 `近期待办.md`），再在「设置 → 导入」里选择 `prototype/todo-import.json`。
- **导入 Seeker 的投递**：运行 `pnpm import:seeker`（读 `~/Documents/Seeker/README.md` 里的投递表），再在「设置 → 导入」里选择 `prototype/jobs-import.json`。
- **从 Flomo 搬笔记**：把 Flomo 导出的文件夹放进 `prototype/`（名字以 `flomo-` 开头），运行 `pnpm import:flomo`，再在「设置 → 导入」里选择 `prototype/notes-import.json`。
- **导出**：「设置 → 导出…」，所有数据存成一个文件，适合长期保存或换电脑。
- **备份和恢复**：App 每天自动备份一次；「设置 → 备份」里可以立即备份，或从任意一份恢复。想异地保存，就把备份文件夹里的文件复制走。
- **开发**：运行 `pnpm app:dev`，用的是测试数据，不影响日常数据。

## 文档

| 文档 | 内容 |
|---|---|
| [开发路线](docs/roadmap.md) | 做到哪了、下一步做什么 |
| [需求](docs/requirements.md) | 已经确定的需求 |
| [原件](docs/archive/) | 两轮需求问卷的答卷（Word）、两份优化清单 |
| [技术架构](docs/architecture.md) | 技术选型、数据存放、备份与升级 |
| [数据结构](docs/schema.md) | 每张表的字段、Excel 怎么导入 |
| [视觉规范](docs/design.md) | 颜色、字体、布局、文案 |
| [CLAUDE.md](CLAUDE.md) | 给 AI 编程助手的工作规则和代码地图 |

## 数据在哪里

- 真实数据：`~/Library/Application Support/Pomelo OS/`
- 开发用的测试数据：`~/Library/Application Support/Pomelo OS Dev/`
- Excel 原型：`prototype/`（只在本机，不进 Git）
