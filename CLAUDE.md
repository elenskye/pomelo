# CLAUDE.md

Pomelo OS（柚儿园）：只给我一个人用的 macOS 本地个人系统。Tauri 2 + React + TypeScript + Rust + SQLite。

## 开工前先读

1. `docs/roadmap.md`：做到哪了、下一步、正式版装到了哪个版本
2. `docs/requirements.md`：已确定的需求（按模块分节，读要动的那几节）
3. 按需再读：`docs/schema.md`（表和规则）、`docs/design.md`（视觉）、`docs/architecture.md`（存放、备份、安全、小拾的做法）

## 最高原则

- **个人、轻量**：只在一台 Mac 上给一个人用。不做账号、协作、云端、跨平台，不做「以后可能用得上」的抽象。
- **按阶段推进**：只做当前阶段的事。阶段结束时功能必须真正能用，并说明验收结果。
- **重要决定先问我**：数据结构、页面结构、新增依赖、功能范围，给出推荐方案，由我来定（对话里问，选项写清楚后果）。大范围的需求用 Word 问卷（放 `docs/`，答完移到 `docs/archive/`，结论写进 `requirements.md`）。

## 工作方式

- **优化清单**：我把意见写在 `docs/pomelo-improve-N.rtf`（用 `textutil -convert txt -stdout` 读），也可能直接在对话里给。逐条做完后移到 `docs/archive/`，在 `roadmap.md` 里更新。
- **汇报**：中文、简洁：每条改了什么、我自己选的地方、旧数据会怎么变、怎么验证的。
- **正式版**：`pnpm app:install` 只在我同意时执行；装之前先做升级测试（见下）。
- **Git**：不主动提交，我自己提交。
- **代码格式**：仓库没有格式化配置，照周围的写法手动排（长行）；不要跑 prettier 这类格式化工具。

## 数据规则

- 只有 Rust 读写 SQLite；界面和小拾只调用 Rust 命令。业务规则只写在 Rust 里。
- **会改数据的命令都用 `state.with_undo("标签", …)`**（小拾自己的数据用 `with_picky_undo`），⌘Z 才能撤销；只读的用 `with_db`。每天的例行检查（`daily_sweep`）、后台自己跑的（定时任务、今日简报）不进撤销，可撤销的编辑也不碰它们记的东西（否则 ⌘Z 会被挡住）。
- 一次改好几张表时用 `common::Tx::begin(conn)?`（可嵌套，基于 SAVEPOINT），不要用 `unchecked_transaction`。
- 改结构只能新增 migration，已有的永远不改；重建有子表的表时注意外键动作（参考 `0018`、`0019`）。每个模块导出再导入必须完全一致（有测试）；数据文件格式变了就把 `transfer.rs` 的版本加一，旧文件要能读。
- 开发和测试只用开发数据目录或临时目录。除非我明确要求，绝不读写真实数据目录 `~/Library/Application Support/Pomelo OS/`。
- 个人数据（xlsx、数据库、备份、导出文件、`prototype/`）不进 Git。
- `search.db` 只放从数据算出来的东西，随时能删了重建：不为它写 migration（改结构就把 `search/store.rs` 的版本加一），不进备份、导出和撤销。新增一类记录要搜，就在 `search/sources.rs` 加一行。
- **建出来的模块**（侧栏加号，`custom.rs`）是数据不是代码：键 `list/编号`、`progress/编号`，编号不重用；隐藏、发给云端、搜索、小拾查数据都按单个模块算。新增读数据的地方（搜索、首页、小拾）要连它们一起照顾。
- **芝麻开门**：密码只以密文存（规则见 `modules/vault.rs` 开头）。不要把它的命令做成 AI 工具，不要加进导出、全局搜索和搜索索引，不要向我要主密码；测试只用临时目录和自己编的测试主密码，不往真实钥匙串写东西（单元测试用 `secure.rs` 的假实现）。

## 个人网站（相关项目）

- `~/Documents/GitHub/personal-website` 是独立项目（Astro + Keystatic，`main` 推到 GitHub 后 Vercel 自动上线），有自己的 `AGENTS.md`、`ROADMAP.md`、`DEPLOY.md`：碰它之前先读。它的规矩：Agent 不审查、不代拟网站内容；push、PR、部署要站主授权；方案获批不等于开工。
- 在 Pomelo 的会话里只读它；要改网站项目里的东西（代码、文档）先问我。接进 Pomelo 怎么做等第八轮问卷（`docs/Questionnaire-round8.docx`）定。

## 小拾（AI）

- 小拾的数据只在 `picky.db`（自己的升级脚本和版本号）；不为小拾给 `pomelo.db` 加 migration。
- **运行方式**由 `picky::mode` 决定：只有正式数据目录的打包版（或 `POMELO_REAL_MODELS=1`）是真实的；开发版、测试、设了 `POMELO_DATA_DIR` 的打包版都是模拟（模拟模型、假的流式服务、Key 在内存，不调 `llmctl.sh`、不连 8080 / 8081 / 8082 和云端、不读写钥匙串）。
- **测试模型**：平时只用模拟的。真的本地模型和真接口照下面「验证方法」，用之前先问我；本地模型是云端的备用，平时也要用它测好。**云端模型平时由我自己在正式版里测**；我让你用云端测时，Key 放在我给的文件里（`POMELO_CLOUD_KEY_FILE`），测试代码从不读钥匙串。
- **本地模型**：`~/Documents/LLM` 由你（Claude）维护：部署模型、整理脚本和清单、保持轻量。改之前说清楚改什么，下载或删除模型文件先问我；只用它自己的文件夹和 launchd 标签，改完在它的 `INSTALLATION.md` 里写现状。聊天模型的上下文：14B（及看不出大小的）最多 16K，9B 这样更小的最多 32K（`models.json`、`llmctl.sh`、Pomelo 都会拦；向量、重排模型不管）。某项功能超出本地模型的能力时，以我的需求为准保留，不降级。
- **签名**：打包用钥匙串里的自签名证书 `Pomelo Self-Signed`（`tauri.conf.json`），更新后钥匙串里的 Key 才还能读；不要改回临时签名。
- **永远不做成 AI 工具**：启停模型、芝麻开门、导入导出、备份恢复、写文件（保存为 Word、改文件都由我点；改文件不留备份）；也没有删除的工具。云端 Key 和插件库的 Key（Tavily、TMDb）只在钥匙串，不进数据库、日志、导出。
- **查数据**：只调用各模块的只读函数；能不能读、这次能不能发给云端都在 Rust 里判断（芝麻开门、隐藏的模块挡掉），界面只传当前页面和记录编号，不传记录内容。合计和统计只写一份：查询和页面共用 Rust 函数。
- **思考**：每次调用模型都传要不要思考（`Options.thinking`，谁看哪个开关见 `requirements.md`）；模型能不能做到看 `cloud::Think` 和模型清单，在 `provider.rs` 里处理；开思考时用 `Route::with_thinking` / `context::think_room` 多留回复长度。
- **写数据**：模型只能提议，Rust 用模块自己的校验试写（在撤回的事务里）后存成卡片，我确认后才写，整张卡片一步 `with_undo`。定时任务、今日简报、年度总结不给写的工具。
- **插件库**（`picky/skills/`）：技能是 Rust 写的固定功能。发给 Wikidata / Open Library 的请求带着我在插件库填的联系邮箱（`picky.db` 偏好 `skills.contact`，不写进代码），别的地方不带。
- **定时任务、今日简报、年度总结**：只读。定时任务和发消息走同一条路（没有界面在听）；今日简报和年度总结不用查数据的工具，Rust 先用 `data.rs` 读好再发。开机自动启动（LaunchAgent）只有正式数据目录的打包版会登记：测试不要碰真实的登录项。
- 模拟模型和 `mockIpc.ts` 都要能模拟查数据、提议写数据、定时任务、简报和年度总结；按意思搜用模拟的向量和重排，联网搜索、书单 / 影单检索用模拟的结果。

## 界面规则

- 不写使用提示（快捷键、怎么操作、撤销提醒、输入框示例）；列表行只放最必要的信息。细则见 `docs/design.md`。
- 图表用 `src/shared/charts.tsx`，颜色只用 `--chart-*`；新增模块颜色要同时写进 `index.css` 和 `design.md`。
- 能滚动的区域用 `-m-1 … p-1`，焦点框才不会被切掉。
- 字体只用 `font-sans`（`index.css` 里的 `Pomelo Hand`），不要直接写 `Hannotate SC`，否则粗体会被系统描粗发糊。小拾消息正文用苹方是唯一的例外（见 `design.md`）。
- 面板里小拾的操作（贴 JD、拆成子任务）放在那一栏标题的右边：`Field` 的 `action` 加 `Assist.tsx` 的 `AssistAction`。

## 文档规则

- **给我看的**：`README.md`、`CLAUDE.md`、`docs/`。简洁、通俗、全中文，只写现状和决定，不写变更历史。
- **不给我看的**：代码、注释、脚本、提交信息，按 Agent 效率最高的方式写。
- 阶段完成或一轮优化后更新 `roadmap.md`；需求变了改 `requirements.md`，数据结构变了改 `schema.md`（含版本记录），视觉变了改 `design.md`，做法变了改 `architecture.md`。

## 代码地图

| 位置 | 内容 |
|---|---|
| `src-tauri/src/modules/` | 每个模块一个文件：类型、校验、SQL、命令（`todo` `finance` `jobs` `notes` `days` `vault` `books` `films` `drawing` `packing`），文件开头是规则摘要；`attachments.rs` 附件（笔记、任务、目标共用）；`custom.rs` 五花八门和循序渐进建出来的模块（键 `list/编号`、`progress/编号`），`lists.rs` 五花八门的字段和条目，`progress.rs` 循序渐进的课程和课节；`kinds.rs` 可自定义的类型；`common.rs` 校验、日期、排序、链接、重读重看、`Tx` |
| `src-tauri/src/search/` | `query.rs` 唯一的搜索入口（按字 / 按意思 / 合并后重排）、`mod.rs` ⌘K 的按字部分、`store.rs` search.db、`sources.rs` 每类记录进索引的字段、`text.rs` 分词分段（改了把 `RULES` 加一）、`vectors.rs` 向量检索、`indexer.rs` 后台同步 |
| `src-tauri/src/picky/` 模型 | `mod.rs` 运行方式、状态、命令和后台循环；`runtime.rs` 本地模型和向量、重排服务（`llmctl.sh`，含模拟）；`catalog.rs` 模型清单；`cloud.rs` 云端模型、思考能力（`Think`）和 Key；`provider.rs` 流式连接（OpenAI 兼容和 Anthropic 两种接口，含模拟模型；发给本地模型的请求在这里排队）；`embed.rs` · `rerank.rs` 向量（8081）、重排（8082） |
| `src-tauri/src/picky/` 对话 | `store.rs` picky.db 和升级脚本；`chat.rs` 对话、版本、生成和查数据的循环（每个对话一个生成位置：`Runs`）；`context.rs` 上下文预算和总结；`library.rs` 对话列表（项目、置顶、定时、私密、归档、拖动）；`instructions.rs` 四层指令和预设；`templates.rs`；`memory.rs` 记忆；`files.rs` · `pdf.rs` · `office.rs` · `reading.rs` · `index.rs` 附件、项目资料（Word、PowerPoint、表格和「查表」、网页、图片）、分段阅读、资料检索；`knowledge.rs` 知识库（库、文件夹和网页、扫描、建索引、检索；文件清单和向量在 search.db）；`work.rs` Work（保存为 Word、改文件和它的卡片），`word.rs` 写 Word、读 Word 段落、保留格式地改；`find.rs` 对话搜索；`export.rs`；`transfer.rs` 数据文件里的小拾部分；`storage.rs` 空间管理 |
| `src-tauri/src/picky/` 能力 | `data.rs` 查 Pomelo 数据（每个模块的只读查询、读不读得到、简报和年度总结要读的）；`cite.rs` 出处和「搜索」「读整条」；`act/` 写数据的工具和确认卡片；`assist.rs` 润色、补全、一句话填好；`assistants/` 助手（`translate.rs` 翻译）；`skills/` 插件库（`works.rs` 书单 / 影单检索，`web.rs` 联网搜索，`files.rs` 读文件和长资料的目录、挑段落，`toolbox.rs` Word 工具箱、文本工具箱：改文件、新建 Word；查知识库在 `knowledge.rs`）；`schedule.rs` 定时任务；`brief.rs` 今日简报；`yearly.rs` 年度总结 |
| `src-tauri/src/` 其他 | `tray.rs` 菜单栏；`autostart.rs` 开机自动启动；`undo.rs` 撤销 / 重做（SQLite session，分 pomelo.db 和 picky.db）；`secure.rs` 钥匙串、Touch ID、剪贴板（测试里换成假的）；`transfer.rs` · `backup.rs` 导出导入、备份恢复；`db.rs` · `migrations/` 连接和升级脚本；`home.rs` 首页汇总 |
| `src/app/` | `App.tsx` 外壳、⌘Z / ⌘K / ⌘J、每天的例行检查；`Sidebar.tsx`；`Search.tsx` 全局搜索；`pageContext.ts` 告诉小窗当前页面；`hidden.ts`；`modules.ts` |
| `src/home/` | 首页；`BriefCard.tsx` 今日简报；`YearCard.tsx` 新年第一周的年度总结 |
| `src/modules/<模块>/` | 各模块页面；书影共用 `src/shared/collection/`；`custom/` 五花八门、循序渐进的页面和新建对话框，`useAllModules` 是固定模块加建出来的模块 |
| `src/modules/picky/` | `PickyPage.tsx` 小拾页；`ConversationList.tsx` 对话列表；`Transcript.tsx` · `markdown.ts` 消息区；`useChat.tsx` 小拾页和小窗共用的流式回复；`SideChat.tsx` 右侧小窗；`Composer.tsx` · `slots.ts` 输入框和「/」菜单；`Manage.tsx` 管理；`Schedules.tsx` 定时；`Plugins.tsx` 插件库；`Knowledge.tsx` 知识库（管理对话框、输入框按钮、项目选库）；`Work.tsx` 改文件、新建 Word 的卡片和保存为 Word；`Assist.tsx` 各模块面板里的润色、补全、一句话填好；`assistants/` 助手和它们的页面（`Translate.tsx`）；`Thinking.tsx` 输入框的思考按钮、模型能不能思考 |
| `src/settings/` | 设置页；`layout.tsx` 带 (?) 说明的行；`Picky.tsx` 小拾和云端模型；`Startup.tsx` 开机自动启动 |
| `src/shared/` | `attachments/`（附件的小图、看大图、拖入和粘贴）、`Dropdown`、`DateInput`、`MonthInput`、`panel`（右侧面板）、`Modal`、`sortable`、`charts`、`settings`（记住界面状态）、`kinds/`、`feedback`、`dates`、`diff`、`InlineMd`、`open`（打开某条记录：页面用 `useOpenRequest` 接住）、`tags`（选项和类型的彩色标签、表格里点了就换） |
| `src/dev/mockIpc.ts` | 浏览器测试用的模拟后端（新命令要加进来） |
| `scripts/` | `install-app.sh` 安装正式版 |

## 新增一个模块

1. 新增 migration，加进 `db.rs` 的 `MIGRATIONS`
2. 写 `src-tauri/src/modules/xxx.rs`（参考 `jobs.rs`），在 `mod.rs`、`lib.rs` 注册；写导出再导入的往返测试
3. 需要「类型」：在 `kinds.rs` 的 `TABLES` 加一行（列名必须叫 `kind_id`），默认值写进 migration
4. `transfer.rs` 加一个 `Payload`（含 `kind_modules`），格式版本加一；`search/sources.rs` 加要搜的字段，页面用 `useOpenRequest` 打开搜到的记录；要上首页的在 `home.rs` 加一块；小拾要能查的在 `picky/data.rs` 加查询
5. 前端：`src/modules/xxx/`，在 `app/modules.ts`、`main.tsx` 注册；模块颜色写进 `index.css`；命令和示例数据加进 `mockIpc.ts`；页面用 `usePageContext` 告诉小窗在哪、打开了哪条
6. 更新文档

侧栏加号建的清单、进度不走这一步。

## 常用命令

| 命令 | 作用 |
|---|---|
| `pnpm check` | 类型检查 + Rust 测试，每次改完都要跑 |
| `pnpm app:dev` | 运行开发版 |
| `pnpm app:build` | 只打包，不安装 |
| `pnpm app:install` | 打包并安装到「应用程序」，只在我同意时执行 |

## 验证方法

- **界面**：用 Bash 后台跑 `pnpm dev`（预览面板没有「文稿」权限，自己启动不了），再用 `.claude/launch.json` 的 `ui-mock` 打开 `http://localhost:1420/?mock`（内存里的模拟数据）。仓库里改任何文件（文档也算）页面都会刷新、模拟数据清空，等结果时别动文件。参数：`&notes=prototype` 真实的 179 条笔记；`&vault=new|open`（默认锁着，测试主密码 `mock-master-1`）、`&touch=fail|none`；`&picky=ready` 一打开就有模拟的本地模型；`&kb=demo` 先放两个知识库；`&brief=fail|off` 今日简报；`&year=card` 首页任何一天都显示年度总结卡片；问题里写「滚动测试」「失败测试」得到长回复 / 失败；小拾选文件第二次多选拿到 xlsx、pptx、png，第一次是 pdf、md；想直接看某页用 `/?mock#/finance`。截图常慢一拍，关键结果用 JS 读页面确认；拖拽用 `left_click_drag` 测。
- **真实窗口**：`pnpm app:build` 后 `open -n --env POMELO_DATA_DIR=<临时目录> "src-tauri/target/release/bundle/macos/Pomelo OS.app"`，用 `screencapture -x` 截全屏看（我在用别的 App 时，新窗口可能被台前调度缩成左边的缩略图，不是 bug）；查数据用 `sqlite3` 看临时目录。测完 `pkill -f "target/release/bundle"`（别用 osascript quit，会连正式版一起关）并删掉这个 `.app`。**不要用 osascript 碰 System Events**（会弹「辅助功能」授权框），交互在浏览器里测。
- **升级测试（装正式版前做）**：先在正式版二进制里 `grep -c -a` 各 migration 的特征 SQL，确定它在哪个版本（不要读真实数据目录；`picky.db` 的升级脚本也要查）；在临时目录里用 `sqlite3` 依次执行到那个版本的 migration、`PRAGMA user_version=<版本>`、插几条会被新 migration 改动的数据，再用上面的方法打开新打包的 App，检查版本号、`backups/` 里的「升级前」备份和数据。
- **真的本地模型（先问我）**：`~/Documents/LLM/scripts/llmctl.sh start qwen359b`，等 `curl -sf 127.0.0.1:8080/health` 通了再跑，用完马上 `llmctl.sh stop`。
  - 小拾：`POMELO_REAL_LLM=1 POMELO_ASK='问题1|问题2>>追问' cargo test --manifest-path src-tauri/Cargo.toml real_local_model -- --ignored --nocapture`（导入文件加编的数据，在内存里；`POMELO_SHOW=1` 显示查到的原文，`POMELO_AS_CLOUD=1` 按云端的规则，`POMELO_SCHEDULED=1` 按定时任务只读）。一次跑一个问题、给个时限：Qwen3.5 9B 偶尔会想很久停不下来。
  - 润色补全、一句话填好：`real_assist`、`real_quick`；今日简报：`real_brief`（`POMELO_TODAY` 换日期）；翻译助手：`real_translate`（`POMELO_TR_CASE=zh|keywords|long`，一次一个；`POMELO_THINK=0` 不思考，开思考时 9B 两句话也会想几分钟）；长资料补读：`real_read_file`（编的 60 章手册，`POMELO_ASK` 换问题）；Word / 文本工具箱：`real_toolbox`（临时文件夹里编的 Word、Markdown 等 15 种情况，打出调用、卡片和写入后读回的结果；`POMELO_TB_CASE=kb,table…` 挑几个，`POMELO_TB_DUMP=1` 只看读出来的段落；我同意用云端时把 `POMELO_REAL_LLM=1` 换成 `POMELO_CLOUD_KEY_FILE=<放 Key 的文件>`，默认 DeepSeek，`POMELO_CLOUD_SERVICE=openrouter POMELO_CLOUD_MODEL=<模型>` 换 OpenRouter；`real_local_model` 也能这样用云端，只用编的数据）；知识库看图：`real_kb_image`（只能用云端，编一张带字的图）。测试打出云端的原始报错（`[cloud]` 开头），App 里不显示。
  - 向量和重排：`llmctl.sh start embedding`（或 `rerank`）后跑 `real_embedding`、`real_rerank`，用完停；`real_services_start_and_stop`、`real_meaning_search`、`real_knowledge`（知识库：临时目录里编的文件夹，建索引、提问、改文件再问）自己启停（先确认都没在跑）。
- **真接口**：书单 / 影单检索 `POMELO_REAL_WEB=1 cargo test real_works -- --ignored --nocapture`（示例书名，可以直接跑）；小拾的真模型测试加 `POMELO_REAL_WEB=1` 就用真检索；Tavily 要我的 Key，先问我。
- **搜索速度**：`cargo test --release --manifest-path src-tauri/Cargo.toml search_speed -- --ignored --nocapture`（20 万条编的数据：按字 0.1 秒内、按意思 0.3 秒内）。
- **真实数据**：`POMELO_SEED_DB=<临时目录>/pomelo.db cargo test --manifest-path src-tauri/Cargo.toml seed_scratch_db -- --ignored` 生成含全部导入文件的临时数据库；`cargo test -- --ignored --skip seed_scratch_db` 跑真实导入文件的测试。

## 环境

- macOS，Apple M3，18 GB 内存。Node 24、pnpm（`~/Library/pnpm/bin`）、Rust（`~/.cargo/bin`），非交互终端里要先加进 PATH。
- 辅助脚本用 TypeScript（Node），不引入 Python。Word 问卷：拿 `docs/archive/` 里上一轮的 docx 当模板，换掉 `word/document.xml`（勾选框是 `w14:checkbox`），再用 `zip` 打包。
