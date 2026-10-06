# CLAUDE.md

Pomelo OS（柚儿园）：只给我一个人用的 macOS 本地个人系统。Tauri 2 + React + TypeScript + Rust + SQLite。

## 开工前先读

1. `docs/roadmap.md`：做到哪了、下一步做什么
2. `docs/requirements.md`：已确定的需求（问卷和优化清单原件在 `docs/archive/`）
3. 按需再读：`docs/architecture.md`、`docs/schema.md`、`docs/design.md`

## 最高原则

- **个人、轻量**：只在一台 Mac 上给一个人用。不做账号、协作、云端、跨平台，不做「以后可能用得上」的抽象。
- **按阶段推进**：只做当前阶段的事。阶段结束时功能必须真正能用，并说明验收结果。
- **重要决定先问我**：数据结构、页面结构、新增依赖、功能范围，给出推荐方案，由我来定。大范围的需求用 Word 问卷（放 `docs/`，勾选为主，答完移到 `docs/archive/`，结论写进 `requirements.md`）；开发中零散的决策点直接在对话里问。

## 数据规则

- 只有 Rust 读写 SQLite；界面（以后还有 AI）只调用 Rust 命令。业务规则只写在 Rust 里。
- **会改数据的命令都用 `state.with_undo("标签", …)`**，这样 ⌘Z 能撤销；只读的用 `with_db`。每天的例行检查（`daily_sweep`）不进撤销。
- 一次改好几张表时用 `common::Tx::begin(conn)?`（可嵌套的事务，基于 SAVEPOINT），不要用 `unchecked_transaction`：模块之间会互相调用（投石问路 → 一步一步），导入时也在外层事务里。
- 改结构只能新增 migration，已有的永远不改。每个模块的导出再导入必须完全一致（有测试）。
- 开发和测试只用开发数据目录或临时目录。除非我明确要求，绝不读写真实数据目录 `~/Library/Application Support/Pomelo OS/`。
- 个人数据（xlsx、数据库、备份、导出文件）不进 Git。
- **芝麻开门**：密码只以密文存（规则见 `modules/vault.rs` 开头）。不要把它的命令做成 AI 工具，不要加进导出，不要向我要主密码；测试只用临时目录和自己编的测试主密码，不往真实钥匙串写东西（单元测试用 `secure.rs` 的假实现）。

## 界面规则

- 不写使用提示（快捷键、怎么操作、撤销提醒、输入框示例）；列表行只放最必要的信息。细则见 `docs/design.md`。
- 图表用 `src/shared/charts.tsx`，颜色只用 `--chart-*`（已检查过色弱可读性）；新增模块颜色要同时写进 `index.css` 和 `design.md`。

## 文档规则

- **给我看的**：`README.md`、`CLAUDE.md`、`docs/`。简洁、通俗、全中文。
- **不给我看的**：代码、注释、脚本、提交信息，按 Agent 效率最高的方式写。
- 阶段完成后更新 `roadmap.md`；数据结构变了改 `schema.md`，视觉变了改 `design.md`。

## 代码地图

| 位置 | 内容 |
|---|---|
| `src-tauri/src/modules/` | 每个模块一个文件：类型、校验、SQL、命令（`todo` `finance` `jobs` `notes` `vault` `books` `films` `drawing` `packing`）；`kinds.rs` 可自定义的类型（`fin_out` / `fin_in` 是记账分类，`job_channel` 是投递渠道）；`common.rs` 通用校验、日期、排序、链接、`Tx` |
| `src-tauri/src/undo.rs` | 撤销 / 重做（SQLite session 记录每次修改） |
| `src-tauri/src/secure.rs` | 芝麻开门用到的 macOS 功能：钥匙串、Touch ID、剪贴板；测试里换成假的 |
| `src-tauri/src/transfer.rs` · `backup.rs` | 数据文件导出导入 · 备份、恢复、每日备份 |
| `src-tauri/src/db.rs` · `migrations/` | 连接、升级脚本列表 · 升级脚本 |
| `src/modules/<模块>/` | 各模块页面；`todo/` 一步一步、`finance/` 钱来钱往、`jobs/` 投石问路、`notes/` 念念不忘、`vault/` 芝麻开门 |
| `src/settings/` | 设置页；`layout.tsx` 里有带 (?) 说明的行 |
| `src/shared/` | 共用：`collection/`（书单影单整页）、`Dropdown`、`panel`（右侧面板）、`Modal`（弹窗）、`sortable`（拖拽排序，整行可拖）、`charts`（SVG 图表）、`settings`（记住界面状态）、`kinds/`、`feedback`（提示、确认框）、`dates`、`InlineMd` |
| `src/app/` | `App.tsx` 外壳、⌘Z / ⌘⇧Z、每天的例行检查（`daily_sweep`）；`Sidebar.tsx` 侧栏（收起、排序）；`modules.ts` 模块列表 |
| `src/dev/mockIpc.ts` | 浏览器测试用的模拟后端（新命令要加进来） |
| `scripts/` | 一次性转换：Excel、Markdown 待办、Seeker 投递、Flomo 导出 → `prototype/*.json` |

## 新增一个模块

1. 新增 migration，加进 `db.rs` 的 `MIGRATIONS`
2. 写 `src-tauri/src/modules/xxx.rs`（参考 `jobs.rs`），在 `mod.rs`、`lib.rs` 注册；改数据的命令用 `with_undo`；写导出再导入的往返测试
3. 需要「类型」：在 `kinds.rs` 的 `TABLES` 加一行（列名必须叫 `kind_id`），默认值写进 migration
4. `transfer.rs`：加一个 `Payload`（含 `kind_modules`），格式版本加一，旧文件要能读
5. 前端：`src/modules/xxx/`，在 `app/modules.ts`（侧栏自动出现）、`main.tsx` 注册；模块颜色写进 `index.css`；命令和示例数据加进 `mockIpc.ts`
6. 更新文档（roadmap、requirements、schema、design）

## 常用命令

| 命令 | 作用 |
|---|---|
| `pnpm check` | 类型检查 + Rust 测试，每次改完都要跑 |
| `pnpm app:dev` | 运行开发版 |
| `pnpm import:xlsx` · `import:todo` · `import:seeker` · `import:flomo` | Excel / Markdown 待办 / Seeker 投递 / Flomo 导出 → `prototype/*.json`，再到「设置 → 导入」 |
| `pnpm app:build` | 只打包，不安装 |
| `pnpm app:install` | 打包并安装到「应用程序」，只在我同意时执行 |

## 验证方法

- **界面**：后台运行 `pnpm dev`，再用 `.claude/launch.json` 的 `ui-mock` 打开 `http://localhost:1420/?mock`（内存里的模拟数据）。改了 mock 要刷新页面（HMR 不会重置它）。加 `&notes=prototype` 会换成 `prototype/notes-import.json` 里的真实笔记；芝麻开门用 `&vault=new|open` 选开始的状态（默认锁着，测试主密码 `mock-master-1`），`&touch=fail|none` 模拟 Touch ID 失败或没有。预览面板自己启动不了这个目录里的命令（没有「文稿」权限），所以 `pnpm dev` 要用 Bash 后台跑。浏览器面板的截图有时慢一拍，关键结果用 JS 读页面确认。拖拽排序可以用 `left_click_drag` 测（它一步跳到终点，拖拽代码要能接住）。
- **真实窗口**：`pnpm app:build` 后 `open -n --env POMELO_DATA_DIR=<临时目录> "src-tauri/target/release/bundle/macos/Pomelo OS.app"`，测完用 `pkill -f "target/release/bundle"` 关掉（别用 osascript quit，会连正式版一起关）并删掉这个 `.app`。这台 Mac 经常截不了屏（显示器休眠），这时只能靠浏览器和测试。
- **升级测试（装正式版前做）**：在临时目录里用 `sqlite3` 依次执行旧的 migration、`PRAGMA user_version=<正式版的版本>`、插几条要被新 migration 改动的数据，再用上面的方法打开新打包的 App，检查版本号、`backups/` 里的「升级前」备份和数据。正式版是哪个版本：在 App 二进制里 `grep -c -a` 各 migration 的特征 SQL（不要读真实数据目录）。
- **真实数据**：`POMELO_SEED_DB=<临时目录>/pomelo.db cargo test --manifest-path src-tauri/Cargo.toml seed_scratch_db -- --ignored` 生成临时数据库（含 Excel、待办、投递、Flomo 笔记的导入文件）；想直接打开某页，改 `settings` 表的 `ui.last_route`。

## 环境

- macOS，Apple M3，18 GB 内存。Node 24、pnpm（`~/Library/pnpm/bin`）、Rust（`~/.cargo/bin`），非交互终端里要先加进 PATH
- 辅助脚本用 TypeScript（Node），不引入 Python
