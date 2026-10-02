# CLAUDE.md

Pomelo OS（柚儿园）：只给我一个人用的 macOS 本地个人系统。Tauri 2 + React + TypeScript + Rust + SQLite。

## 开工前先读

1. `docs/roadmap.md`：做到哪了、下一步做什么
2. `docs/requirements.md`：已确定的需求（问卷原件在 `docs/archive/`）
3. 按需再读：`docs/architecture.md`、`docs/schema.md`、`docs/design.md`

## 最高原则

- **个人、轻量**：只在一台 Mac 上给一个人用。不做账号、协作、云端、跨平台，不做「以后可能用得上」的抽象。
- **按阶段推进**：只做当前阶段的事。阶段结束时功能必须真正能用，并说明验收结果。
- **重要决定先问我**：数据结构、页面结构、新增依赖、功能范围，给出推荐方案，由我来定。大范围的需求用 Word 问卷（放 `docs/`，勾选为主，答完移到 `docs/archive/`，结论写进 `requirements.md`）；开发中零散的决策点直接在对话里问。

## 数据规则

- 只有 Rust 读写 SQLite；界面（以后还有 AI）只调用 Rust 命令。业务规则只写在 Rust 里。
- **会改数据的命令都用 `state.with_undo("标签", …)`**，这样 ⌘Z 能撤销；只读的用 `with_db`。
- 改结构只能新增 migration，已有的永远不改。每个模块的导出再导入必须完全一致（有测试）。
- 开发和测试只用开发数据目录或临时目录。除非我明确要求，绝不读写真实数据目录 `~/Library/Application Support/Pomelo OS/`。
- 个人数据（xlsx、数据库、备份、导出文件）不进 Git。

## 文档规则

- **给我看的**：`README.md`、`CLAUDE.md`、`docs/`。简洁、通俗、全中文。
- **不给我看的**：代码、注释、脚本、提交信息，按 Agent 效率最高的方式写。
- 阶段完成后更新 `roadmap.md`；数据结构变了改 `schema.md`，视觉变了改 `design.md`。

## 代码地图

| 位置 | 内容 |
|---|---|
| `src-tauri/src/modules/` | 每个模块一个文件：类型、校验、SQL、命令（`todo` `books` `films` `drawing` `packing`）；`kinds.rs` 可自定义的类型；`common.rs` 通用校验、日期计算、排序 |
| `src-tauri/src/undo.rs` | 撤销 / 重做（SQLite session 记录每次修改） |
| `src-tauri/src/transfer.rs` · `backup.rs` | 数据文件导出导入 · 备份、恢复、每日备份 |
| `src-tauri/src/db.rs` · `migrations/` | 连接、升级脚本列表 · 升级脚本 |
| `src/modules/<模块>/` | 各模块页面；`todo/` 是「一步一步」 |
| `src/settings/` | 设置页；`layout.tsx` 里有带 (?) 说明的行 |
| `src/shared/` | 共用：`collection/`（书单影单整页）、`Dropdown`、`panel`（右侧面板）、`sortable`（拖拽排序）、`kinds/`、`feedback`（提示、确认框）、`dates`、`InlineMd` |
| `src/app/App.tsx` | 外壳、⌘Z / ⌘⇧Z、每日备份 |
| `src/dev/mockIpc.ts` | 浏览器测试用的模拟后端（新命令要加进来） |
| `scripts/` | Excel 转换、Markdown 待办转换 |

## 新增一个模块

1. 新增 migration，加进 `db.rs` 的 `MIGRATIONS`
2. 写 `src-tauri/src/modules/xxx.rs`，在 `mod.rs`、`lib.rs` 注册；改数据的命令用 `with_undo`；在 `transfer.rs` 加上导入导出
3. 需要「类型」：在 `kinds.rs` 的 `TABLES` 加一行
4. 前端：`src/modules/xxx/`，在 `app/modules.ts`、`main.tsx` 注册；命令加进 `mockIpc.ts`
5. 更新文档

## 常用命令

| 命令 | 作用 |
|---|---|
| `pnpm check` | 类型检查 + Rust 测试，每次改完都要跑 |
| `pnpm app:dev` | 运行开发版 |
| `pnpm import:xlsx` · `pnpm import:todo` | Excel / Markdown 待办 → `prototype/*.json`，再到「设置 → 导入」 |
| `pnpm app:build` | 只打包，不安装 |
| `pnpm app:install` | 打包并安装到「应用程序」，只在阶段验收后、我同意时执行 |

## 验证方法

- **界面**：后台运行 `pnpm dev`，再用 `.claude/launch.json` 的 `ui-mock` 打开 `http://localhost:1420/?mock`（内存里的模拟数据）。改了 mock 要刷新页面（HMR 不会重置它）。浏览器面板的截图有时慢一拍，关键结果用 JS 读页面确认。拖拽排序可以用 `left_click_drag` 测（它一步跳到终点，拖拽代码要能接住）。
- **真实窗口**：`pnpm app:build` 后 `open -n --env POMELO_DATA_DIR=<临时目录> "src-tauri/target/release/bundle/macos/Pomelo OS.app"`，测完删掉这个 `.app`。这台 Mac 经常截不了屏（显示器休眠），这时只能靠浏览器和测试。
- **真实数据**：`POMELO_SEED_DB=<临时目录>/pomelo.db cargo test --manifest-path src-tauri/Cargo.toml seed_scratch_db -- --ignored` 生成临时数据库（含 Excel 和待办导入文件）；想直接打开某页，改 `settings` 表的 `ui.last_route`。

## 环境

- macOS，Apple M3，18 GB 内存。Node 24、pnpm（`~/Library/pnpm/bin`）、Rust（`~/.cargo/bin`），非交互终端里要先加进 PATH
- 辅助脚本用 TypeScript（Node），不引入 Python
