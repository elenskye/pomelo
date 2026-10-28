# CLAUDE.md

Pomelo OS（柚儿园）：只给我一个人用的 macOS 本地个人系统。Tauri 2 + React + TypeScript + Rust + SQLite。

## 开工前先读

1. `docs/roadmap.md`：做到哪了、下一步从哪开始、正式版装到哪个版本
2. `docs/requirements.md`：已定的需求（按模块分节，读要动的那几节）
3. 按需：`docs/schema.md`（表和规则）、`docs/design.md`（视觉）、`docs/architecture.md`（存放、备份、安全、小拾的做法）、`docs/testing.md`（怎么测）

## 最高原则

- **个人、轻量**：一台 Mac、一个人。不做账号、协作、云端、跨平台，不做「以后可能用得上」的抽象。
- **按阶段推进**：只做当前阶段的事。阶段结束时功能必须真正能用，并说明验收结果。
- **重要决定先问我**：数据结构、页面结构、新增依赖、功能范围，给出推荐方案由我来定（对话里问，写清后果）。大范围的需求用 Word 问卷（放 `docs/`，答完移到 `docs/archive/`，结论写进 `requirements.md`）。界面大改先在 `prototype/ui-rN/` 做本地网页原型给我看。

## 工作方式

- **优化清单**：我写在 `docs/pomelo-improve-N.rtf`（`textutil -convert txt -stdout` 读），也可能直接在对话里给。逐条做完后移到 `docs/archive/`，更新 `roadmap.md`。
- **汇报**：中文、简洁：每条改了什么、我自己选的地方、旧数据会怎么变、怎么验证的。
- **正式版**：`pnpm app:install` 只在我同意时执行，装之前先做升级测试（`docs/testing.md`）。
- **Git**：我说了才提交；提交信息一行，照我的写法（`v0.7: merge seeklo`、`v0.6 refined`）。不推送。
- **代码格式**：没有格式化配置，照周围的写法手动排（长行）；不跑 prettier 这类工具。

## 多个 worktree

- 主目录 `~/Documents/GitHub/pomelo`（main）是我日常用的。大块功能可能在单独的 worktree 里做（也可能 Codex 写第一轮、Claude 审核）；提交、合并、推送都由我来，`pnpm app:install` 只在 main 上、合并以后做。
- 主目录之外跑开发版设 `POMELO_DATA_DIR="$HOME/Library/Application Support/Pomelo OS Dev-<分支名>"`（共用「Pomelo OS Dev」会被分支的 migration 升级）。`pnpm dev` 固定 1420 端口，同时只能一个；每个 worktree 自己 `pnpm install`、有自己的 `src-tauri/target`。
- 分支里新加的 migration 接着现有编号写，合并前 main 也加过就把分支的往后改号（main 上已有的永远不改）；`picky.db` 升级脚本、`transfer.rs` 格式版本、`search/store.rs` 版本同理。

## 数据规则

- 只有 Rust 读写 SQLite；界面和小拾只调用 Rust 命令。业务规则只写在 Rust 里。
- **会改数据的命令都用 `state.with_undo("标签", …)`**（小拾的数据用 `with_picky_undo`），只读的用 `with_db`。例行检查（`daily_sweep`）、后台自己跑的（定时任务、今日简报）和各种设置不进撤销，可撤销的编辑也不碰它们记的东西（否则 ⌘Z 会被挡住）。
- 一次改好几张表用 `common::Tx::begin(conn)?`（可嵌套，基于 SAVEPOINT），不用 `unchecked_transaction`。
- 改结构只能新增 migration，已有的永远不改；重建有子表的表注意外键动作（参考 `0018`、`0019`）。每个模块导出再导入必须完全一致（有测试）；数据文件格式变了就把 `transfer.rs` 的版本加一，旧文件要能读。
- 开发和测试只用开发数据目录或临时目录。除非我明确要求，绝不读写真实数据目录 `~/Library/Application Support/Pomelo OS/`。个人数据（xlsx、数据库、备份、导出文件、`prototype/`）不进 Git。
- `search.db` 只放从数据算出来的东西，随时能删了重建：不写 migration（改结构把 `search/store.rs` 的版本加一），不进备份、导出和撤销。新增一类要搜的记录，在 `search/sources.rs` 加一行。
- **建出来的模块**（五花八门 / 循序渐进，`custom.rs`）是数据不是代码：键 `list/编号`、`progress/编号`，编号不重用；隐藏、发给云端、搜索、小拾查数据都按单个模块算，新增读数据的地方（搜索、首页、小拾）要连它们一起照顾。
- **芝麻开门**：密码只以密文存（规则见 `modules/vault.rs` 开头）。不做成 AI 工具，不进导出、全局搜索和索引，不向我要主密码；测试只用临时目录和自己编的主密码，不往真实钥匙串写（单元测试用 `secure.rs` 的假实现）。

## 个人网站（相关项目）

- `~/Documents/GitHub/personal-website` 是独立项目（Astro，仓库 `elenskye/elenskye-website`，`main` 推上去 Vercel 自动上线），有自己的 `AGENTS.md`、`ROADMAP.md`、`DEPLOY.md`，碰之前先读。它的规矩：Agent 不审查、不代拟网站内容；push、PR、部署要站主授权；方案获批不等于开工。
- 内容只在 Pomelo 的「呦呦漫游」（`site/`）里编辑和发布：只在网站文件夹里，不进 `pomelo.db`、备份、导出、撤销、搜索；内容格式只在 `site/` 的 Rust 里写一份，网站改了格式我会说。
- **真的网站文件夹只读**：只读文件、只用只读的 git 命令（status、log、diff、show），提交、推送、拉取、切分支由我做。开发和测试用副本加本机的假远端（`site::mode`），绝不往真的 GitHub 推。改网站项目里的代码和文档要我说开工，只改文件、不碰 git。

## Seeker（v0.7）

- `~/Documents/Seeker` 是我的求职资料（不是 Git 仓库，有自己的 `AGENTS.md`）。Pomelo 里的「Seeker」是小拾的助手（代码和数据在 `picky.db`），资料以 Pomelo 为准；需求见 `requirements.md` 的「Seeker」「Rater」。
- 真文件夹的边界（`transfer/` 在 v0.7 验收后删了）：`AGENTS.md` 只读；`knowledge/` 只读文件夹、文件名和 `招聘索引.md`；`material/` 只看文件名（Pomelo 也只列文件名，不读内容、不发给模型）；`cv/` 能看文件名，两份模板 `cv/cn/seek-it-cn-template.docx`、`cv/en/seek-it-keyreply-resume-v5.docx` 只读（复制到临时文件夹里测），别的简历要读内容先问我；原件不改、不移、不删。
- 开发和测试只用临时目录里编的资料（照 `seeker/tests.rs`）；开发版打不开真的 Seeker 文件夹（Rust 里拦着）。简历、经历、证书这些个人资料不进 Git，文档和问卷里的例子也用编的。

## 小拾（AI）

- 数据只在 `picky.db`（自己的升级脚本和版本号），不为小拾给 `pomelo.db` 加 migration。
- **锁不能套着用**：`with_db` / `picky.with_db` 的闭包里不能再调用会自己上同一把锁的函数（比如 `picky.selection()`），会卡死整个 App。页面反复调用、可能慢的命令写成 `async`。
- **运行方式**由 `picky::mode` 决定：只有正式数据目录的打包版（或 `POMELO_REAL_MODELS=1`）是真实的；开发版、测试、设了 `POMELO_DATA_DIR` 的打包版都是模拟（模拟模型、假的流式服务、Key 在内存，不调 `llmctl.sh`、不连 8080 / 8081 / 8082 和云端、不碰钥匙串）。
- **测试模型**：平时只用模拟的。真的本地模型和真接口照 `docs/testing.md`，用之前先问我；本地模型是云端的备用，也要测好。云端平时由我在正式版里测；我让你用云端时，Key 在我给的文件里（`POMELO_CLOUD_KEY_FILE`），测试代码从不读钥匙串。
- **本地模型**：`~/Documents/LLM` 由你（Claude）维护：部署、整理脚本和清单、保持轻量。改之前说清楚，下载或删除模型文件先问我；只用它自己的文件夹和 launchd 标签，改完在它的 `INSTALLATION.md` 里写现状。聊天模型上下文：14B（及看不出大小的）最多 16K，9B 这样更小的最多 32K（`models.json`、`llmctl.sh`、Pomelo 都拦）。功能超出本地模型能力时以我的需求为准，不降级。
- **签名**：打包用钥匙串里的自签名证书 `Pomelo Self-Signed`（`tauri.conf.json`），更新后钥匙串里的 Key 才还能读；不要改回临时签名。
- **永远不做成 AI 工具**：启停模型、芝麻开门、导入导出、备份恢复、写文件（保存为 Word、改文件都由我点；改文件不留备份）；也没有删除的工具。云端 Key 和插件库的 Key（Tavily、TMDb）只在钥匙串，不进数据库、日志、导出。
- **查数据**：只调用各模块的只读函数；能不能读、能不能发给云端都在 Rust 里判断（芝麻开门、隐藏的模块挡掉），界面只传当前页面和记录编号。合计和统计只写一份：查询和页面共用 Rust 函数。
- **思考**：每次调用模型都传要不要思考（`Options.thinking`）；模型能不能做到看 `cloud::Think` 和模型清单，在 `provider.rs` 里处理；开思考时用 `Route::with_thinking` / `context::think_room` 多留回复长度。
- **写数据**：模型只能提议，Rust 用模块自己的校验试写（在撤回的事务里）后存成卡片，我确认后才写，整张卡片一步 `with_undo`。
- **插件库**（`picky/skills/`）：技能是 Rust 写的固定功能。发给 Wikidata / Open Library 的请求带我填的联系邮箱（`picky.db` 偏好 `skills.contact`，不写进代码）。
- **助手**（`assistants/`）：名字由我来定，新加的先问我（现在有 Translator、Pub2MD、Seeker、Rater）。
- **定时任务、今日简报、年度总结**：只读，不给写的工具；简报和年度总结也不给查数据的工具（Rust 先用 `data.rs` 读好）。开机自动启动只有正式数据目录的打包版会登记，测试不碰真实的登录项。
- 模拟模型和 `mockIpc.ts` 都要能模拟查数据、提议写数据、定时任务、简报和年度总结；按意思搜用模拟的向量和重排，联网搜索、书单 / 影单检索用模拟的结果。

## 界面规则

- 不写使用提示；列表行只放最必要的信息。细则见 `docs/design.md`。
- 外框（v0.7 阶段 ① 起）：顶栏、页签和前进后退、索引签、模块坞、柚子瓣、柚子，照 `requirements.md`「界面外框」和 `design.md`「外框」；各模块自己的布局不动。
- 图表用 `src/shared/charts.tsx`，颜色只用 `--chart-*`；新增模块颜色要同时写进 `index.css` 和 `design.md`。
- 能滚动的区域用 `-m-1 … p-1`，焦点框才不会被切掉。拖动排序用鼠标事件（`src/shared/sortable`），不用浏览器自带的拖放。
- 字体只用 `font-sans`（`index.css` 里的 `Pomelo Hand`），不直接写 `Hannotate SC`（粗体会被系统描粗发糊）。小拾消息正文用苹方是唯一的例外。
- 面板里小拾的操作（贴 JD、拆成子任务）放在那一栏标题的右边：`Field` 的 `action` 加 `Assist.tsx` 的 `AssistAction`。

## 文档规则

- **给我看的**：`README.md`、`CLAUDE.md`、`AGENTS.md`、`docs/`。简洁、通俗、全中文，只写现状和决定，不写变更历史。
- **不给我看的**：代码、注释、脚本、提交信息，按 Agent 效率最高的方式写。
- 阶段完成或一轮优化后更新 `roadmap.md`；需求变了改 `requirements.md`，数据结构变了改 `schema.md`（含版本记录），视觉变了改 `design.md`，做法变了改 `architecture.md`，测法变了改 `testing.md`。

## 代码地图

| 位置 | 内容 |
|---|---|
| `src-tauri/src/modules/` | 每个模块一个文件（`todo` `finance` `jobs` `notes` `days` `vault` `books` `films` `drawing` `packing`），开头是规则摘要；`attachments.rs` 附件；`custom.rs` · `lists.rs` · `progress.rs` 五花八门和循序渐进；`kinds.rs` 可自定义的类型；`common.rs` 校验、日期、排序、链接、`Tx` |
| `src-tauri/src/search/` | `query.rs` 唯一的搜索入口（按字 / 按意思 / 合并重排）、`mod.rs` ⌘K、`store.rs` search.db、`sources.rs` 每类记录进索引的字段、`text.rs` 分词（改了把 `RULES` 加一）、`vectors.rs`、`indexer.rs` 后台同步 |
| `src-tauri/src/picky/` 模型 | `mod.rs` 运行方式、状态、后台循环；`runtime.rs` 本地模型和向量、重排服务；`catalog.rs` 模型清单；`cloud.rs` 云端模型、`Think`、Key；`provider.rs` 流式连接（OpenAI 兼容、Anthropic、模拟；本地模型排队）；`embed.rs` · `rerank.rs` |
| `src-tauri/src/picky/` 对话 | `store.rs` picky.db 和升级脚本；`chat.rs` 对话和生成循环（每个对话一个 `Runs` 位置）；`context.rs` 上下文预算；`library.rs` 对话列表；`instructions.rs` · `templates.rs` · `memory.rs`；`files.rs` · `pdf.rs` · `office.rs` · `reading.rs` · `index.rs` 附件和资料；`knowledge.rs` 知识库；`work.rs` · `word.rs` 保存为 Word、改文件、照模板写简历；`find.rs` · `export.rs` · `transfer.rs` · `storage.rs` |
| `src-tauri/src/picky/` 能力 | `data.rs` 查 Pomelo 数据；`cite.rs` 出处和「搜索」「读整条」；`act/` 写数据的工具和确认卡片；`assist.rs` 润色、补全、一句话填好；`assistants/`：`seeker.rs` + `seeker/`（`catalog` 资料、`import` 从 transfer/ 导入、`workflow` 组装和版本、`model` 模型建议、`output` Word 导出和岗位关联、`company` knowledge/ 的公司和检索、`files` cv/ 和 material/、`tests`），`rater.rs` Rater 的题库 + `rater/qbank.rs` 三个题库 SKILL + `rater/practice.rs` 卡片练习 + `rater/interview.rs` 面试，`translate.rs`，`site.rs` 呦呦漫游的按中文起草，`pub2md/`（流程在 `job.rs`）；`skills/` 插件库（`seeker.rs` 只读的「Seeker 资料」，`qbank.rs` 对话里的题库 SKILL）；`schedule.rs` 定时任务；`brief.rs` 今日简报；`yearly.rs` 年度总结 |
| `src-tauri/src/site/` | 呦呦漫游：`mod.rs` 运行方式、副本、命令；`tools.rs` git / gh / Node；`server.rs` 本地服务；`changes.rs` · `publish.rs` 改动、发布；`yaml.rs`（照 js-yaml 写，`yaml_cases.rs` 对照）· `doc.rs` 内容文件；`wander.rs` `now.rs` `about.rs` `career.rs` 各页；`registry.rs` · `images.rs` 资源库；`terms.rs` 话题、类别、外链 |
| `src-tauri/src/` 其他 | `dock.rs` 模块坞的实时小字；`tray.rs` 菜单栏；`autostart.rs`；`undo.rs` 撤销 / 重做（SQLite session，分两个库）；`secure.rs` 钥匙串、Touch ID、剪贴板；`transfer.rs` · `backup.rs` 导出导入、备份恢复；`db.rs` · `migrations/`；`home.rs` 首页汇总 |
| `src/app/` | `App.tsx` 外壳、⌘Z / ⌘K / ⌘J、例行检查；`TopBar.tsx` 顶栏；`tabs.tsx` 页签和前进后退、`subpage.ts` 子页进地址；`IndexTabs.tsx` 索引签；`dock.ts` · `DockPage.tsx` 模块坞；`Bloom.tsx` 柚子瓣；`Pomelo.tsx` 柚子的表情和状态；`drag.tsx` · `Menu.tsx`；`frame.css`；`Search.tsx`；`pageContext.ts`；`hidden.ts`；`modules.ts`（模块、助手、子页） |
| `src/home/` | 首页、`BriefCard.tsx`、`YearCard.tsx` |
| `src/modules/<模块>/` | 各模块页面；书影共用 `src/shared/collection/`；`custom/` 五花八门、循序渐进；`site/` 呦呦漫游（`SitePage.tsx` 入口和页签、`parts.tsx` 共用的、`MdEditor.tsx`）；`jobs/SeekerExports.tsx` · `jobs/Interviews.tsx` 岗位面板里的 Seeker 导出和面试 |
| `src/modules/picky/` | `PickyPage.tsx`；`ConversationList.tsx`；`Transcript.tsx` · `markdown.ts`；`useChat.tsx`；`SideChat.tsx` 右侧小窗；`Composer.tsx` · `slots.ts`；`Manage.tsx` `Schedules.tsx` `Plugins.tsx` `Knowledge.tsx` `Work.tsx`；`Assist.tsx` 面板里的帮手；`Thinking.tsx`；`assistants/`（`Seeker.tsx` 和 `Seeker*.tsx`、`seekerApi.ts`、`seekerDraft.ts`；`SeekerFiles.tsx` cv/ 和 material/；`Rater.tsx` · `RaterCards.tsx` 卡片页 · `RaterInterview.tsx` 面试页 · `raterApi.ts`；`Translate.tsx`；`Pub2md.tsx` · `MmdPreview.tsx`） |
| `src/settings/` · `src/shared/` | 设置页；共用组件（`Dropdown` `DateInput` `panel` `Modal` `sortable` `charts` `editable` `diff` `InlineMd` `open` `tags` `attachments/` 等） |
| `src/dev/mockIpc.ts` | 浏览器测试用的模拟后端（新命令要加进来；Seeker 的在 `seekerMock.ts`） |
| `scripts/` · `prototype/` | `install-app.sh`、`clean-build.sh`；界面原型 `prototype/ui-rN/`（不进 Git，现在是 `ui-r4`） |

## 新增一个模块

1. 新增 migration，加进 `db.rs` 的 `MIGRATIONS`
2. 写 `src-tauri/src/modules/xxx.rs`（参考 `jobs.rs`），在 `mod.rs`、`lib.rs` 注册；写导出再导入的往返测试
3. 需要「类型」：在 `kinds.rs` 的 `TABLES` 加一行（列名必须叫 `kind_id`），默认值写进 migration
4. `transfer.rs` 加一个 `Payload`（含 `kind_modules`），格式版本加一；`search/sources.rs` 加要搜的字段，页面用 `useOpenRequest` 打开搜到的记录；要上首页的在 `home.rs` 加一块；小拾要能查的在 `picky/data.rs` 加查询
5. 前端：`src/modules/xxx/`，在 `app/modules.ts`、`main.tsx` 注册，`app/dock.ts` 的 `DEFAULT` 里定它默认在模块坞哪个分类；有子页的在 `modules.ts` 写 `subs`、页面用 `useSubPage`；模块颜色写进 `index.css`；命令和示例数据加进 `mockIpc.ts`；页面用 `usePageContext` 告诉小窗在哪、打开了哪条
6. 更新文档

五花八门、循序渐进建的模块不走这一步。

## 常用命令和验证

| 命令 | 作用 |
|---|---|
| `pnpm check` | 类型检查 + Rust 测试，每次改完都要跑 |
| `pnpm app:dev` · `pnpm app:build` | 运行开发版 · 只打包不安装 |
| `pnpm app:install` | 打包并安装到「应用程序」，只在我同意时执行 |
| `pnpm clean:build` | 清编译缓存 `src-tauri/target`（超过 20 GB 时各命令和开会话的 hook 会自动清；别改回带调试信息的编译设置） |

- 怎么测都在 `docs/testing.md`：界面用 Bash 后台跑 `pnpm dev`、再开 `ui-mock` 的 `http://localhost:1420/?mock`；`?mock` 不走 Rust，新加或改了 Rust 命令的页面还要在打包版里用临时数据目录点一次（不用 osascript 碰 System Events）。
- 装正式版前做升级测试；真的本地模型、云端、联网接口、Pub2MD 真测都先问我。

## 环境

- macOS，Apple M3，18 GB 内存。Node 24、pnpm（`~/Library/pnpm/bin`）、Rust（`~/.cargo/bin`），非交互终端里要先加进 PATH。
- 辅助脚本用 TypeScript（Node），不引入 Python。Word 问卷：拿 `docs/archive/` 里上一轮的 docx 当模板，换掉 `word/document.xml`（勾选框是 `w14:checkbox`），再用 `zip` 打包。
