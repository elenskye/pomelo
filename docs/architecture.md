# 技术架构

## 基本原则

- **个人、轻量**：只给我一个人在一台 Mac 上用。不做账号、多人协作、云端服务、跨平台，也不搭复杂的架构。
- **够用就好**：能用简单办法解决的，不用复杂办法。每个阶段先保证真正能用，再往下做。
- **数据能长期保存、随时带走**：App 可以随时换掉，数据不会丢。

## 技术选型

| 部分 | 选择 |
|---|---|
| 桌面应用外壳 | Tauri 2 |
| 界面 | React + TypeScript + Vite，React Router，TanStack Query，Tailwind CSS + shadcn/ui |
| 数据层 | Rust + rusqlite（自带 SQLite） |
| 包管理 | pnpm（前端），cargo（Rust） |
| 辅助脚本 | TypeScript（Node），不引入 Python 环境 |

## 分层

```
界面（React） ──┐
               ├── 调用命令 ──▶ Rust 命令层 ──▶ SQLite
AI 助手（以后）──┘              业务规则、校验、事务都在这一层
```

- 只有 Rust 端读写数据库。界面不写 SQL，只调用 `books_list`、`books_update` 这样的命令。
- 业务规则只写在命令层一处，比如「标为已读时，自动填上完成时间」。
- 以后 AI 用的 Pomelo Tools 就是这些命令的一层说明，AI 永远碰不到 SQL。

## 数据存放在哪里

| 位置 | 放什么 | 要不要备份 |
|---|---|---|
| `/Applications/Pomelo OS.app` | App 本体 | 不用，可随时重装 |
| `~/Library/Application Support/Pomelo OS/` | 个人数据根目录 | **要** |
| 　├ `pomelo.db` | 唯一的主数据库（包括设置） | 要 |
| 　├ `backups/` | 自动和手动备份 | 这里就是备份 |
| 　└ `attachments/` | 附件（v0.1 不用） | 要 |
| `~/Library/Application Support/Pomelo OS Dev/` | 开发时用的数据，和真实数据完全分开 | 不用 |
| `~/Library/Caches/` 下的缓存、本地大模型 | 可以重新下载的东西 | 不用 |
| macOS 钥匙串 | API Key 等密钥 | 不进数据目录，也不进备份 |

- 不要把数据库放进 iCloud Drive、Dropbox 这类同步文件夹，同步过程可能把数据库弄坏。想异地保存，就把**备份文件**复制过去。

## 数据库升级（Migration）

- 每次改数据库结构，都新增一个编号脚本（`src-tauri/migrations/0001_xxx.sql` …），脚本编译进 App。
- App 启动时检查数据库版本：如果需要升级，**先自动备份**，再逐个执行升级脚本。
- 已经用过的升级脚本永远不改，只新增。
- 如果数据库版本比 App 还新（比如装回了旧版 App），App 会拒绝打开，防止把数据写坏。

## 备份与导入导出

| | 快照备份 | JSON 导出 |
|---|---|---|
| 是什么 | 整个数据库的完整副本 | 所有模块的数据，人能看懂 |
| 用来做什么 | 出问题时一键恢复 | 长期归档，或换到别的工具 |
| 什么时候做 | 手动；每天第一次打开 App；每次升级前 | 手动 |
| 怎么恢复 | 替换当前数据库（替换前会先备份当前的） | 导入 |

- v0.1 的导入只支持「导入到空模块」和「覆盖整个模块」，不做合并。
- Excel 原型也走 JSON 导入：先用脚本把 `柚儿园.xlsx` 转成 Pomelo 的导出格式，再用 App 导入。

## AI 助手（以后再做，v0.1 不涉及）

- Pomelo 自己的 AI 助手运行在界面里，能接 OpenAI、Claude 和本地模型（Ollama），三种模型共用同一套 Pomelo Tools。
- 会修改数据的 Tool，执行前要我在界面上确认。
- 已有的 pub2md-agent（PDF / Markdown 翻译工具，Python + Django，本地运行）保持独立，不重写。以后作为「本地服务」接进 Pomelo，具体方式到 AI 阶段再定。
- pub2md-agent 已经在用 OpenAI 兼容接口。Pomelo 的模型接入也以 OpenAI 兼容接口为主（Ollama 同样支持），Claude 单独接入。
