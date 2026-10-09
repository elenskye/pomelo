# CLAUDE.md

Pomelo OS（柚儿园）：只给我一个人用的 macOS 本地个人系统。Tauri 2 + React + TypeScript + Rust + SQLite。

## 开工前先读

1. `docs/roadmap.md`：做到哪了、下一步、正式版装到了哪个版本
2. `docs/requirements.md`：已确定的需求
3. 按需再读：`docs/schema.md`（表和规则）、`docs/design.md`（视觉）、`docs/architecture.md`（存放、备份、安全）

## 最高原则

- **个人、轻量**：只在一台 Mac 上给一个人用。不做账号、协作、云端、跨平台，不做「以后可能用得上」的抽象。
- **按阶段推进**：只做当前阶段的事。阶段结束时功能必须真正能用，并说明验收结果。
- **重要决定先问我**：数据结构、页面结构、新增依赖、功能范围，给出推荐方案，由我来定（对话里问，选项写清楚后果）。大范围的需求用 Word 问卷（放 `docs/`，答完移到 `docs/archive/`，结论写进 `requirements.md`）。

## 工作方式

- **优化清单**：我把意见写在 `docs/pomelo-improve-N.rtf`（用 `textutil -convert txt -stdout` 读）。逐条做完后移到 `docs/archive/`，在 `roadmap.md` 的进度里更新一句。
- **汇报**：中文、简洁：每条改了什么、我自己选的地方、旧数据会怎么变、怎么验证的。
- **正式版**：`pnpm app:install` 只在我同意时执行；装之前先做升级测试（见下）。
- **Git**：不主动提交，我自己提交。

## 数据规则

- 只有 Rust 读写 SQLite；界面（以后还有 AI）只调用 Rust 命令。业务规则只写在 Rust 里。
- **会改数据的命令都用 `state.with_undo("标签", …)`**，⌘Z 才能撤销；只读的用 `with_db`。每天的例行检查（`daily_sweep`）不进撤销。
- 一次改好几张表时用 `common::Tx::begin(conn)?`（可嵌套，基于 SAVEPOINT），不要用 `unchecked_transaction`。
- 改结构只能新增 migration，已有的永远不改；重建有子表的表时注意外键动作（参考 `0018`、`0019`）。每个模块导出再导入必须完全一致（有测试）；数据文件格式变了就把 `transfer.rs` 的版本加一，旧文件要能读。
- 开发和测试只用开发数据目录或临时目录。除非我明确要求，绝不读写真实数据目录 `~/Library/Application Support/Pomelo OS/`。
- 个人数据（xlsx、数据库、备份、导出文件、`prototype/`）不进 Git。
- **芝麻开门**：密码只以密文存（规则见 `modules/vault.rs` 开头）。不要把它的命令做成 AI 工具，不要加进导出和全局搜索，不要向我要主密码；测试只用临时目录和自己编的测试主密码，不往真实钥匙串写东西（单元测试用 `secure.rs` 的假实现）。

## 界面规则

- 不写使用提示（快捷键、怎么操作、撤销提醒、输入框示例）；列表行只放最必要的信息。细则见 `docs/design.md`。
- 图表用 `src/shared/charts.tsx`，颜色只用 `--chart-*`；新增模块颜色要同时写进 `index.css` 和 `design.md`。
- 能滚动的区域用 `-m-1 … p-1`，焦点框才不会被切掉。
- 字体只用 `font-sans`（`index.css` 里的 `Pomelo Hand`），不要直接写 `Hannotate SC`，否则粗体会被系统描粗发糊。

## 文档规则

- **给我看的**：`README.md`、`CLAUDE.md`、`docs/`。简洁、通俗、全中文，只写现状和决定，不写变更历史。
- **不给我看的**：代码、注释、脚本、提交信息，按 Agent 效率最高的方式写。
- 阶段完成或一轮优化后更新 `roadmap.md`；需求变了改 `requirements.md`，数据结构变了改 `schema.md`（含版本记录），视觉变了改 `design.md`。

## 代码地图

| 位置 | 内容 |
|---|---|
| `src-tauri/src/modules/` | 每个模块一个文件：类型、校验、SQL、命令（`todo` `finance` `jobs` `notes` `vault` `books` `films` `drawing` `packing`），文件开头是规则摘要；`kinds.rs` 可自定义的类型；`common.rs` 校验、日期、排序、链接、重读重看、`Tx` |
| `src-tauri/src/search.rs` · `home.rs` | 全局搜索：每个模块搜哪些字段（芝麻开门不搜）· 首页的汇总（一个只读命令） |
| `src-tauri/src/undo.rs` | 撤销 / 重做（SQLite session） |
| `src-tauri/src/secure.rs` | 芝麻开门用到的 macOS 功能：钥匙串、Touch ID、剪贴板；测试里换成假的 |
| `src-tauri/src/transfer.rs` · `backup.rs` | 数据文件导出导入 · 备份、恢复、每日备份 |
| `src-tauri/src/db.rs` · `migrations/` | 连接、升级脚本列表（含升级测试）· 升级脚本 |
| `src/home/` | 首页（今天的待办直接用一步一步的组件） |
| `src/app/` | `App.tsx` 外壳、⌘Z / ⌘K、每天的例行检查；`Sidebar.tsx` 侧栏；`hidden.ts` 隐藏的模块；`Search.tsx` 全局搜索框；`modules.ts` 模块列表 |
| `src/modules/<模块>/` | 各模块页面（`todo/` `finance/` `jobs/` `notes/` `vault/` `drawing/` `packing/`）；书影共用 `src/shared/collection/`，`books/` `films/` 里只有配置 |
| `src/settings/` | 设置页；`layout.tsx` 里有带 (?) 说明的行 |
| `src/shared/` | 共用：`Dropdown`、`DateInput`、`MonthInput`（年月 / 年）、`panel`（右侧面板）、`Modal`、`sortable`、`charts`、`settings`（记住界面状态）、`kinds/`、`feedback`、`dates`、`InlineMd`、`open`（从全局搜索、首页打开某条记录：页面用 `useOpenRequest` 接住） |
| `src/dev/mockIpc.ts` | 浏览器测试用的模拟后端（新命令要加进来） |
| `scripts/` | 一次性转换：Excel、Markdown 待办、Seeker 投递、Flomo 导出 → `prototype/*.json` |

## 新增一个模块

1. 新增 migration，加进 `db.rs` 的 `MIGRATIONS`
2. 写 `src-tauri/src/modules/xxx.rs`（参考 `jobs.rs`），在 `mod.rs`、`lib.rs` 注册；写导出再导入的往返测试
3. 需要「类型」：在 `kinds.rs` 的 `TABLES` 加一行（列名必须叫 `kind_id`），默认值写进 migration
4. `transfer.rs` 加一个 `Payload`（含 `kind_modules`），格式版本加一；`search.rs` 加要搜的字段，页面用 `useOpenRequest` 打开搜到的记录；要上首页的在 `home.rs` 加一块
5. 前端：`src/modules/xxx/`，在 `app/modules.ts`、`main.tsx` 注册；模块颜色写进 `index.css`；命令和示例数据加进 `mockIpc.ts`
6. 更新文档

## 常用命令

| 命令 | 作用 |
|---|---|
| `pnpm check` | 类型检查 + Rust 测试，每次改完都要跑 |
| `pnpm app:dev` | 运行开发版 |
| `pnpm app:build` | 只打包，不安装 |
| `pnpm app:install` | 打包并安装到「应用程序」，只在我同意时执行 |
| `pnpm import:xlsx` · `import:todo` · `import:seeker` · `import:flomo` | 一次性转换 → `prototype/*.json`，再到「设置 → 导入」 |

## 验证方法

- **界面**：用 Bash 后台跑 `pnpm dev`（预览面板没有「文稿」权限，自己启动不了），再用 `.claude/launch.json` 的 `ui-mock` 打开 `http://localhost:1420/?mock`（内存里的模拟数据，改了 mock 要刷新页面）。`&notes=prototype` 换成真实的 179 条笔记；芝麻开门用 `&vault=new|open`（默认锁着，测试主密码 `mock-master-1`）、`&touch=fail|none`。截图常慢一拍，面板没显示时截不了图，关键结果用 JS 读页面确认；拖拽用 `left_click_drag` 测。
- **真实窗口**：`pnpm app:build` 后 `open -n --env POMELO_DATA_DIR=<临时目录> "src-tauri/target/release/bundle/macos/Pomelo OS.app"`，用 `screencapture` 截图看；测完 `pkill -f "target/release/bundle"`（别用 osascript quit，会连正式版一起关）并删掉这个 `.app`。**不要用 osascript 碰 System Events**（发键盘事件、查窗口编号都会弹系统「辅助功能」授权框），截图用 `screencapture -x` 截全屏，交互在浏览器里测。
- **升级测试（装正式版前做）**：先在正式版二进制里 `grep -c -a` 各 migration 的特征 SQL，确定它在哪个版本（不要读真实数据目录）；在临时目录里用 `sqlite3` 依次执行到那个版本的 migration、`PRAGMA user_version=<版本>`、插几条会被新 migration 改动的数据，再用上面的方法打开新打包的 App，检查版本号、`backups/` 里的「升级前」备份和数据。
- **真实数据**：`POMELO_SEED_DB=<临时目录>/pomelo.db cargo test --manifest-path src-tauri/Cargo.toml seed_scratch_db -- --ignored` 生成含全部导入文件的临时数据库；`cargo test -- --ignored --skip seed_scratch_db` 跑真实导入文件的测试。打开 App 总是首页；浏览器里想直接看某页用 `/?mock#/finance`。

## 环境

- macOS，Apple M3，18 GB 内存。Node 24、pnpm（`~/Library/pnpm/bin`）、Rust（`~/.cargo/bin`），非交互终端里要先加进 PATH。
- 辅助脚本用 TypeScript（Node），不引入 Python。
