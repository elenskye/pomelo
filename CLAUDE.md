# CLAUDE.md

Pomelo OS（柚儿园）：只给我一个人用的 macOS 本地个人系统。Tauri 2 + React + TypeScript + Rust + SQLite。

## 开工前先读

1. `docs/roadmap.md`：做到哪了、下一步做什么
2. `docs/requirements.md`：已确定的需求（问卷原件在 `docs/archive/`）
3. 按需再读：`docs/architecture.md`、`docs/schema.md`、`docs/design.md`

## 最高原则

- **个人、轻量**：只在一台 Mac 上给一个人用。不做账号、多人协作、云端服务、跨平台，不搭复杂架构，不做「以后可能用得上」的抽象。
- **按阶段推进**：只做当前阶段的事。阶段结束时功能必须真正能用，并说明验收结果。
- **重要决定先问我**：数据结构、页面结构、新增依赖、功能范围。给出推荐方案，由我来定。
- **怎么问**：大范围、整个模块的需求，用 Word 问卷（放 `docs/`，勾选为主，答完移到 `docs/archive/`，结论写进 `requirements.md`）；开发过程中零散的几个决策点，直接在对话里提问。

## 数据规则

- 只有 Rust 端读写 SQLite。界面（以及以后的 AI 助手）只调用 Rust 命令，不写 SQL。业务规则只写在 Rust 里。
- 改数据库结构只能新增 migration；已有的 migration 永远不改。
- 开发和测试只用开发数据目录或临时目录。除非我明确要求，绝不读写真实数据目录 `~/Library/Application Support/Pomelo OS/`。
- 个人数据（xlsx、数据库、备份、导出文件）不进 Git。`prototype/柚儿园.xlsx` 只读。

## 文档规则

- **给我看的**：根目录的 `README.md`、`CLAUDE.md` 和 `docs/`。要简洁、通俗、全中文。
- **不给我看的**：代码、注释、脚本、提交信息。按 Agent 最容易理解、效率最高的方式写。
- 每个阶段完成后更新 `docs/roadmap.md`；数据结构变了改 `docs/schema.md`，视觉变了改 `docs/design.md`。

## 代码地图

| 位置 | 内容 |
|---|---|
| `src-tauri/src/modules/` | 每个模块一个文件：类型、校验、SQL、Tauri 命令（`todo` `books` `films` `drawing` `packing`）；`kinds.rs` 是可自定义的类型 / 类别；`common.rs` 是状态和通用校验 |
| `src-tauri/src/transfer.rs` | Pomelo 数据文件的导出和导入 |
| `src-tauri/src/backup.rs` | 快照备份：列表、立即备份、每日备份和保留规则、恢复 |
| `src-tauri/src/db.rs` · `migrations/` | 数据库连接、升级脚本列表 · 升级脚本 |
| `src/modules/<模块>/` | 各模块页面 |
| `src/settings/` | 设置页：导出导入（`Transfer`）、备份（`Backups`） |
| `src/shared/` | 共用：`collection/`（书单、影单共用的整页）、`Dropdown`、`panel`（右侧编辑面板）、`kinds/`（管理类型）、`NameDialog`、`feedback`（提示和确认框）、`dates`（本地日期）、`InlineMd`（**加粗**） |
| `src/dev/mockIpc.ts` | 浏览器测试用的模拟后端 |
| `scripts/xlsx-to-pomelo/` · `scripts/md-todo-to-pomelo/` | Excel 转换脚本 · Markdown 待办转换脚本 |

## 新增一个模块

1. 新增 `src-tauri/migrations/000N_xxx.sql`，加进 `db.rs` 的 `MIGRATIONS`
2. 写 `src-tauri/src/modules/xxx.rs`（参考 `drawing.rs`、`packing.rs`），在 `mod.rs`、`lib.rs` 注册；在 `transfer.rs` 加上导入和导出（导出再导入必须完全一致，有测试把关）
3. 需要「类型」：在 `kinds.rs` 的 `TABLES` 加一行，默认类型写进 migration
4. 前端：`src/modules/xxx/`，在 `app/modules.ts`、`main.tsx` 注册；再把命令加进 `src/dev/mockIpc.ts`
5. 更新文档

## 常用命令

| 命令 | 作用 |
|---|---|
| `pnpm check` | 类型检查 + Rust 测试，每次改完都要跑 |
| `pnpm app:dev` | 运行开发版 |
| `pnpm import:xlsx` | Excel → `prototype/pomelo-import.json` |
| `pnpm import:todo` | Markdown 待办 → `prototype/todo-import.json` |
| `pnpm app:build` | 只打包，不安装 |
| `pnpm app:install` | 打包并安装到「应用程序」，只在阶段验收后、我同意时执行 |

## 验证方法

- **界面交互**：后台运行 `pnpm dev`，再用 `.claude/launch.json` 的 `ui-mock` 连上，打开 `http://localhost:1420/?mock`。用的是内存里的模拟数据，可以随便点。浏览器面板自己启动服务器会遇到权限问题，所以要先在终端里跑起来。
- **真实窗口**：`pnpm app:build` 后用 `open -n --env POMELO_DATA_DIR=<临时目录> "src-tauri/target/release/bundle/macos/Pomelo OS.app"` 打开，不会碰真实数据。测完删掉这个 `.app`，否则「应用程序」列表里会多出一个。
- **真实数据截图**：先生成临时数据库 `POMELO_SEED_DB=<临时目录>/pomelo.db cargo test --manifest-path src-tauri/Cargo.toml seed_scratch_db -- --ignored`；想直接打开某一页，就改临时数据库 `settings` 表里的 `ui.last_route`（如 `"/packing"`）。
- **限制**：屏幕锁定时，真实窗口只能截图、不能点击；后台工具点不开真实窗口里的下拉菜单。这些只能在浏览器里测。

## 环境

- macOS，Apple M3，18 GB 内存。已装 Node 24、pnpm（`~/Library/pnpm/bin`）、Rust（`~/.cargo/bin`），非交互终端里要先加进 PATH
- 辅助脚本用 TypeScript（Node），不引入 Python
