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

## 小拾（AI）

- 需求见 `requirements.md` 的「小拾」，进度见 `roadmap.md`。原来的 Picky 已停用，它的项目和数据都删了。
- 小拾的数据只在 `picky.db`（自己的升级脚本和版本号）；不为小拾给 `pomelo.db` 加 migration。
- 开发和测试只用模拟模型和假的流式服务（用 TS 或 Rust 写，不用 Picky 的 Python 脚本）：不调 `llmctl.sh` / `embeddingctl.sh`，不连真实的 8080 / 8081 和云端，不读写钥匙串；要用真模型先问我。`~/Documents/LLM` 里的东西一律不改。运行方式由 `picky::mode` 决定：只有正式数据目录的打包版（或 `POMELO_REAL_MODELS=1`）是真实的，开发版、测试、设了 `POMELO_DATA_DIR` 的打包版都是模拟。
- 打包用钥匙串里的自签名证书 `Pomelo Self-Signed` 签名（`tauri.conf.json`），这样更新后钥匙串里的 Key 还能读；不要改回临时签名。
- 启停模型、芝麻开门的命令永远不做成 AI 工具；云端 Key 只在钥匙串，不进数据库、日志、导出。
- 小拾查数据（v0.3.5 起）只调用各模块的只读函数，任何写数据的命令都不做成工具；能不能读、这次能不能发给云端都在 Rust 里判断（芝麻开门、隐藏的模块挡掉），界面只传当前页面和记录编号，不传记录内容。合计和统计只写一份：查询和页面共用 Rust 函数，还在界面里算的先搬进 Rust。模拟模型和 `mockIpc.ts` 都要能模拟查数据。

## 界面规则

- 不写使用提示（快捷键、怎么操作、撤销提醒、输入框示例）；列表行只放最必要的信息。细则见 `docs/design.md`。
- 图表用 `src/shared/charts.tsx`，颜色只用 `--chart-*`；新增模块颜色要同时写进 `index.css` 和 `design.md`。
- 能滚动的区域用 `-m-1 … p-1`，焦点框才不会被切掉。
- 字体只用 `font-sans`（`index.css` 里的 `Pomelo Hand`），不要直接写 `Hannotate SC`，否则粗体会被系统描粗发糊。小拾消息正文用苹方是唯一的例外（见 `design.md`）。

## 文档规则

- **给我看的**：`README.md`、`CLAUDE.md`、`docs/`。简洁、通俗、全中文，只写现状和决定，不写变更历史。
- **不给我看的**：代码、注释、脚本、提交信息，按 Agent 效率最高的方式写。
- 阶段完成或一轮优化后更新 `roadmap.md`；需求变了改 `requirements.md`，数据结构变了改 `schema.md`（含版本记录），视觉变了改 `design.md`。

## 代码地图

| 位置 | 内容 |
|---|---|
| `src-tauri/src/modules/` | 每个模块一个文件：类型、校验、SQL、命令（`todo` `finance` `jobs` `notes` `vault` `books` `films` `drawing` `packing`），文件开头是规则摘要；`kinds.rs` 可自定义的类型；`common.rs` 校验、日期、排序、链接、重读重看、`Tx` |
| `src-tauri/src/search.rs` · `home.rs` | 全局搜索：每个模块搜哪些字段（芝麻开门不搜）· 首页的汇总（一个只读命令） |
| `src-tauri/src/picky/` | 小拾：`store.rs` picky.db 和它的升级脚本、`runtime.rs` 本地模型和向量服务（llmctl / embeddingctl，含模拟）、`catalog.rs` 模型清单、`cloud.rs` 云端模型和 Key、`provider.rs` 流式连接、`chat.rs` 对话、版本和生成、`library.rs` 列表整理（项目、置顶、归档、拖动）、`find.rs` 搜索（也给 ⌘K）、`export.rs` 导出、`templates.rs` 模板、`instructions.rs` 四层指令和用途预设、`context.rs` 上下文预算和总结、`files.rs` 附件和项目资料（副本、取文字、版本）、`pdf.rs` PDFKit 取文字、`reading.rs` 附件和资料怎么发（放不下就分段阅读）、`data.rs` 查 Pomelo 数据（每个模块的只读查询、能不能读和发给云端、查了什么）、`embed.rs` 向量服务（8081，含模拟）、`index.rs` 资料索引和检索、`memory.rs` 记忆（范围、挑选、/记住）、`transfer.rs` 数据文件里的小拾部分、`storage.rs` 空间管理、`assist.rs` 编辑面板里的润色、补全和一句话填好（日期词由 Rust 算）；`mod.rs` 运行方式、状态和命令。前端 `src/modules/picky/`：`markdown.ts` 消息排版和公式、`Transcript.tsx` 消息区、`useChat.tsx` 小拾页和小窗共用的流式回复和记忆检索、`SideChat.tsx` 右侧小窗、`Assist.tsx` 润色、补全、一句话填好（各模块面板用；改动对比在 `src/shared/diff.ts`）、`Composer.tsx` · `slots.ts` 输入框（「/」菜单、待填位置） |
| `src-tauri/src/tray.rs` | 菜单栏图标和菜单（关窗口不退出） |
| `src-tauri/src/undo.rs` | 撤销 / 重做（SQLite session；记得每一步属于 pomelo.db 还是 picky.db） |
| `src-tauri/src/secure.rs` | 芝麻开门用到的 macOS 功能：钥匙串、Touch ID、剪贴板；测试里换成假的 |
| `src-tauri/src/transfer.rs` · `backup.rs` | 数据文件导出导入 · 备份、恢复、每日备份 |
| `src-tauri/src/db.rs` · `migrations/` | 连接、升级脚本列表（含升级测试）· 升级脚本 |
| `src/home/` | 首页（今天的待办直接用一步一步的组件） |
| `src/app/` | `App.tsx` 外壳、⌘Z / ⌘K / ⌘J（小拾小窗）、每天的例行检查；`pageContext.ts` 各页面告诉小窗当前页面（只传编号）；`Sidebar.tsx` 侧栏；`hidden.ts` 隐藏的模块；`Search.tsx` 全局搜索框；`modules.ts` 模块列表 |
| `src/modules/<模块>/` | 各模块页面（`todo/` `finance/` `jobs/` `notes/` `vault/` `drawing/` `packing/` `picky/`）；书影共用 `src/shared/collection/`，`books/` `films/` 里只有配置 |
| `src/settings/` | 设置页；`layout.tsx` 里有带 (?) 说明的行；`Picky.tsx` 小拾的设置，`PickyStorage.tsx` 空间管理 |
| `src/shared/` | 共用：`Dropdown`、`DateInput`、`MonthInput`（年月 / 年）、`panel`（右侧面板）、`Modal`、`sortable`、`charts`、`settings`（记住界面状态）、`kinds/`、`feedback`、`dates`、`InlineMd`、`open`（从全局搜索、首页打开某条记录：页面用 `useOpenRequest` 接住） |
| `src/dev/mockIpc.ts` | 浏览器测试用的模拟后端（新命令要加进来） |
| `scripts/` | `install-app.sh` 安装正式版；`flomo-to-pomelo/` Flomo 导出 → `prototype/notes-import.json`（以后补回 Flomo 的图片和录音还要用，靠记录时间对上笔记） |

## 新增一个模块

1. 新增 migration，加进 `db.rs` 的 `MIGRATIONS`
2. 写 `src-tauri/src/modules/xxx.rs`（参考 `jobs.rs`），在 `mod.rs`、`lib.rs` 注册；写导出再导入的往返测试
3. 需要「类型」：在 `kinds.rs` 的 `TABLES` 加一行（列名必须叫 `kind_id`），默认值写进 migration
4. `transfer.rs` 加一个 `Payload`（含 `kind_modules`），格式版本加一；`search.rs` 加要搜的字段，页面用 `useOpenRequest` 打开搜到的记录；要上首页的在 `home.rs` 加一块；小拾要能查的在 `picky/data.rs` 加查询和记录内容
5. 前端：`src/modules/xxx/`，在 `app/modules.ts`、`main.tsx` 注册；模块颜色写进 `index.css`；命令和示例数据加进 `mockIpc.ts`；页面用 `usePageContext` 告诉小窗在哪、打开了哪条
6. 更新文档

## 常用命令

| 命令 | 作用 |
|---|---|
| `pnpm check` | 类型检查 + Rust 测试，每次改完都要跑 |
| `pnpm app:dev` | 运行开发版 |
| `pnpm app:build` | 只打包，不安装 |
| `pnpm app:install` | 打包并安装到「应用程序」，只在我同意时执行 |
| `pnpm import:flomo` | Flomo 导出 → `prototype/notes-import.json`，再到「设置 → 导入」 |

## 验证方法

- **界面**：用 Bash 后台跑 `pnpm dev`（预览面板没有「文稿」权限，自己启动不了），再用 `.claude/launch.json` 的 `ui-mock` 打开 `http://localhost:1420/?mock`（内存里的模拟数据，改了 mock 要刷新页面）。`&notes=prototype` 换成真实的 179 条笔记；芝麻开门用 `&vault=new|open`（默认锁着，测试主密码 `mock-master-1`）、`&touch=fail|none`；小拾用 `&picky=ready`（一打开就有模拟的本地模型），问题里写「滚动测试」「失败测试」得到长回复 / 失败。截图常慢一拍，面板没显示时截不了图，关键结果用 JS 读页面确认；拖拽用 `left_click_drag` 测。
- **真实窗口**：`pnpm app:build` 后 `open -n --env POMELO_DATA_DIR=<临时目录> "src-tauri/target/release/bundle/macos/Pomelo OS.app"`，用 `screencapture` 截图看；测完 `pkill -f "target/release/bundle"`（别用 osascript quit，会连正式版一起关）并删掉这个 `.app`。**不要用 osascript 碰 System Events**（发键盘事件、查窗口编号都会弹系统「辅助功能」授权框），截图用 `screencapture -x` 截全屏，交互在浏览器里测。
- **升级测试（装正式版前做）**：先在正式版二进制里 `grep -c -a` 各 migration 的特征 SQL，确定它在哪个版本（不要读真实数据目录；`picky.db` 的升级脚本也要查）；在临时目录里用 `sqlite3` 依次执行到那个版本的 migration、`PRAGMA user_version=<版本>`、插几条会被新 migration 改动的数据，再用上面的方法打开新打包的 App，检查版本号、`backups/` 里的「升级前」备份和数据。
- **真模型（先问我）**：`llmctl.sh start qwen359b` 后 `POMELO_REAL_LLM=1 POMELO_ASK='问题1|问题2>>追问' cargo test --manifest-path src-tauri/Cargo.toml real_local_model -- --ignored --nocapture`（导入文件加编的数据，在内存里；`POMELO_SHOW=1` 显示查到的原文，`POMELO_AS_CLOUD=1` 按云端的规则）；润色补全、一句话填好换成 `real_assist`、`real_quick`。启动后等 `curl -sf 127.0.0.1:8080/health` 通了再跑，用完马上 `llmctl.sh stop`。
- **真实数据**：`POMELO_SEED_DB=<临时目录>/pomelo.db cargo test --manifest-path src-tauri/Cargo.toml seed_scratch_db -- --ignored` 生成含全部导入文件的临时数据库；`cargo test -- --ignored --skip seed_scratch_db` 跑真实导入文件的测试。打开 App 总是首页；浏览器里想直接看某页用 `/?mock#/finance`。

## 环境

- macOS，Apple M3，18 GB 内存。Node 24、pnpm（`~/Library/pnpm/bin`）、Rust（`~/.cargo/bin`），非交互终端里要先加进 PATH。
- 辅助脚本用 TypeScript（Node），不引入 Python。
