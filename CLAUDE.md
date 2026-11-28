# CLAUDE.md

Pomelo OS（柚儿园）：只给我一个人用的 macOS 本地个人系统。Tauri 2 + React + TypeScript + Rust + SQLite。

## 开工前

1. **先问我这次属于哪种程度的开发**（见「版本」），定了版本号再动手。
2. 读 `HANDOFF.md`（交接：现在到哪、正式版装到哪个版本、没做完的、等我做的）、`docs/roadmap.md`（以后做什么、做完的版本）和 `docs/requirements.md` 里要动的那几节。
3. 按需读：`docs/schema.md`（表和规则）、`docs/design.md`（视觉、共用的摆法和控件）、`docs/architecture.md`（存放、备份、安全、柚子的做法）、`docs/testing.md`（怎么测）。
4. 这次要用到 visual-atelier（改视觉、同步方案、用 `dw` 看画布、改 Atelier 本身），开工前先读它的 `CLAUDE.md`（见「visual-atelier」）。

## 版本

- 版本号 `主.次.补`（现在到哪见 `HANDOFF.md`）。小幅优化、修 Bug 升最后一位（1.0.0 → 1.0.1）；新功能、大幅优化升中间一位（1.0.0 → 1.1.0）；升第一位（→ 2.0.0）由我决定。
- 升中间一位、第一位的分 Phase 计划（比如 1.2.0 Phase 1～5），写进 `roadmap.md`，每个 Phase 做完功能要真能用并说明验收结果；升最后一位的不分 Phase。
- 开新版本时把 `package.json`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json` 三处版本号（设置「关于」里显示的）一起改。

## 最高原则

- **个人、轻量**：一台 Mac、一个人。不做账号、协作、云端、跨平台，不做「以后可能用得上」的抽象。只做当前版本（或 Phase）的事。
- **重要决定先问我**：数据结构、页面结构、新增依赖、功能范围，给推荐方案、写清后果，由我来定。一两个决定用提问功能问；大范围的需求用问卷页（照 visual-atelier 的 `questionnaire` Skill 做，源文件放 visual-atelier 的 `canvases/pomelo-<名字>/`，发成 Artifact，答案存在页面存储里读回；结论写进 `requirements.md` 后问我删不删）。界面大改先在 `prototype/ui-rN/` 做本地网页原型给我看。

## 工作方式

- **优化清单**：我写在 `docs/pomelo-improve-N.rtf`（`textutil -convert txt -stdout` 读）或直接在对话里给。做完删掉清单，更新 `roadmap.md`。
- **交接**：根目录的 `HANDOFF.md` 是每轮开发的交接文档，不是日志，也不是开发计划：只写现状、没做完的、等我做的、要留意的，让新会话或新 Agent 不用我提醒就能接手。每轮收尾都更新（做完的删掉），汇报里说改了哪几条。
- **汇报**：中文、简洁：每条改了什么、我自己选的地方、旧数据会怎么变、怎么验证的。
- **能直接改的**：这边的文档、代码、`package.json`、版本号。
- **正式版**（`pnpm app:install`，装之前先做升级测试，见 `docs/testing.md`）：升最后一位的（1.0.0 → 1.0.1）做完直接装；升中间一位、第一位的（→ 1.1.0、→ 2.0.0）只在最后一轮收尾时自动装，中间的 Phase 我说要装才装（我也可能自己装）。
- **Git**：在 main 上做，每一轮开发你自己提交（不用问，可以分几个），推送由我来做。提交信息全英文（不夹中文），一行 `vX.Y.Z: 概括`，越短越好（`v1.0.1: fix some problems`、`v1.1.0: minor improvement` 都行），不署名（不加 Co-Authored-By 之类）。每轮汇报列出这一轮的提交：哈希前 7 位、提交信息、各改了什么。
- **只在主目录做**：`~/Documents/GitHub/pomelo`（main）是我日常用的，不开 worktree；`pnpm app:install` 只在这里做。`pnpm dev` 固定 1420 端口，同时只能一个。
- **代码格式**：没有格式化配置，照周围的写法手动排（长行）；不跑 prettier 这类工具。

## visual-atelier（视觉）

- 方案、资产和原型在 `~/Documents/GitHub/visual-atelier`（`schemes/pomelo/`，规则在它的 `DESIGN.md`）。**用到它就先读它的 `CLAUDE.md`**；在那边做事照它的规矩：版本、Git、提交信息（`v<Atelier 的版本>: …`）、tag、几个会话一起做，不照本文件的「版本」和「Git」。那边还有别的会话（Atelier 自己的、Peck、Pith），工作区里别人没提交的改动别动、别还原，只提交自己改的。
- **全部权限**（2026-11-27 我定）：这边的会话对 Atelier 有全部权限：Atelier 本体（`bench/`、共用的 `tooling/`、`skills/`、`docs/`、根目录的规则和配置）和 Pomelo 负责的那块（`schemes/pomelo/`、`canvases/pomelo-*/`、`tooling/atlas/`、`tooling/handmade/`）都能直接编辑、优化、更新，照那边的规矩提交、打 tag，不用问、不用另开会话；用的时候发现 Atelier 不顺就回那边改，记进它的 `CHANGELOG.md`。只有两件先问我：改别的项目的方案（Pith、Peck、Vermilion）；给 Atelier 加新功能（修小毛病、改顺手的直接做）。细则见 Atelier `CLAUDE.md`「几个会话怎么分」。方案自己的版本记在 `schemes/pomelo/scheme.json`、tag `pomelo-v<版本>`，和 App 的版本号无关。
- **新设计**：先在 Atelier 的本地画布上出样子（Pomelo Explore 加一页，照它的 `design-loop` Skill），我在 bench 或「有板有眼」里批；Atelier 的画布和 Design System 默认只在本地，我说了才发到云端（Atelier `CLAUDE.md`「画布与 bench」；问卷页照它的 `questionnaire` Skill 发）。批了源进 `canvases/pomelo-<名字>/`、资产进 `schemes/pomelo/`，再发版、提交、打 tag、核对 tag、同步。方案发版、Design System「Pomelo 柚儿园」的更新、同步进来都由这边的会话做。
- **同步来的资产**在 `src/vendor/pomelo/`（单向，核对 tag 后 `sync`，不手改，要改回那边改）：`pomelo.css`、`ds.css`、`tokens.ts`、手画零件 `handmade.js`、`marks/`（柚子的表情、模块贴纸、界面小图标、空状态插图、菜单栏和 App 图标……）、Atlas 的设计快照、`VERSION`、`SHA256SUMS`。根元素的 `data-palette`、`data-theme` 由 `src/shared/appearance.ts` 写，`dark:` 只看 `data-theme`。
- **图标**：界面小图标从 `@/shared/icons` 取（名字沿用 Lucide 的 `Plus`、`Trash2`…；没有也不装 lucide-react）。新图标回那边 `schemes/pomelo/marks/icons.mjs` 画、发版同步，再在 `icons.tsx` 加一行；贴纸、空状态插图同理（那边的 `marks/`）。柚子的表情、模块贴纸用 `src/shared/marks.tsx` 的 `Mark`、`moduleMark`（每次换一套 id，同一张能放很多次）。
- **Atlas 的设计是冻结的快照**（`src/vendor/pomelo/atlas/<名字>-<版本>/`），不改。只升最后一位的小修替掉同一条线里旧的那份（照旧的做的改照新的画）；升了前两位的两份都留（旧的几份照旧的画）。新设计：那边定稿（名字和版本号我定）、冻结、同步，再改 `youzi/atlas.rs` 的 `DESIGN`。「呈现结果」只能加，旧的行和快照都要读得了。年度小报只有我能删（没有删它的 AI 工具，后台任务也不删）。

## 引擎

- Pomelo 用到的引擎（`src-tauri/Cargo.toml` 里依赖它，或当期版本正在接它；隔几层的也算，如 engine-pdf 用的 engine-core）在 `~/Documents/GitHub/personal-engine-studio/engine-*`。这边的会话可以直接改它们、照那边的规矩提交和打 tag，不用问、不用另开会话；动之前读它的 `CLAUDE.md`、`HANDOFF.md`、`ROADMAP.md`。
- 细则（改已有接口先问我、版本号、开发时怎么指本机路径、push 先后）见引擎 `CLAUDE.md`「二」末尾「改用到的引擎」（总仓库 `docs/decisions/0009`）。没用到的引擎只读。
- **本机已有的大文件**（2026-11-28 我定）：别的仓库里已经有的大文件（预编译库、模型这类，不进 Git）要在这边用时，用 APFS 克隆（`cp -c`）放一份到这边不进 Git 的文件夹：同一块盘上不多占空间，这边自己就能编，那边删了也不影响。不软链、不跨仓库指路径、不重新下载。现在有的：PDFium 静态库（`scripts/pdfium.sh` 克隆到 `src-tauri/vendor/pdfium/`，`.cargo/config.toml` 指过去）。

## 数据规则

- 只有 Rust 读写 SQLite；界面和柚子只调用 Rust 命令，业务规则只写在 Rust 里。
- **会改数据的命令都用 `state.with_undo("标签", …)`**（柚子的数据用 `with_youzi_undo`），只读的用 `with_db`。例行检查（`daily_sweep`）、后台自己跑的（定时任务、今日简报）和各种设置不进撤销，可撤销的编辑也不碰它们记的东西（否则 ⌘Z 会被挡住）。
- 一次改几张表用 `common::Tx::begin(conn)?`（可嵌套，基于 SAVEPOINT），不用 `unchecked_transaction`。
- **只加不改**：改结构只能新增 migration；migration、`youzi.db` 升级脚本、`transfer.rs` 格式版本、`search/store.rs` 版本都接着现有编号写，已有的永远不改。重建有子表的表注意外键动作（参考 `0018`、`0019`）。每个模块导出再导入必须完全一致（有测试）；数据文件格式变了 `transfer.rs` 的版本加一，旧文件要能读。
- **真实数据目录** `~/Library/Application Support/Pomelo OS/`：开发和测试只用开发数据目录或临时目录，除非我明确要求绝不读写。我允许时也只碰我说的那几块：读用只读拷贝（`sqlite3 -readonly <库> ".backup <临时文件>"`），写之前先问，绝不撤回或覆盖我在正式版里做的修改。个人数据（xlsx、数据库、备份、导出文件、`prototype/`）不进 Git。
- **算出来的东西**随时能删了重建，不写 migration，不进备份、导出和撤销（算出来的库用 `db::Derived` 打开）：`search.db`（改结构把 `search/store.rs` 的版本加一；新一类要搜的记录在 `search/sources.rs` 加一行）、`ocr.db`（知识库扫描件识别出的字，`youzi/scans.rs`；改结构把它的版本加一）、`janitor.db`（见「DevJanitor」）、一网打尽的网站图标（数据目录 `icons/`，`icons.rs`；只有正式数据目录的打包版联网去取，开发版和测试只显示首字母）。
- **建出来的模块**（五花八门 / 循序渐进，`custom.rs`）是数据不是代码：键 `list/编号`、`progress/编号`，编号不重用；隐藏、发给云端、搜索、柚子查数据都按单个模块算，新增读数据的地方（搜索、首页、柚子）要连它们一起照顾。
- **芝麻开门**：密码只以密文存（规则见 `modules/vault.rs` 开头）。不做成 AI 工具，不进导出、全局搜索和索引，不向我要主密码；测试只用临时目录和自己编的主密码，不往真实钥匙串写（单元测试用 `secure.rs` 的假实现）。

## 个人网站

- `~/Documents/GitHub/personal-website`：独立项目（Astro，仓库 `elenskye/elenskye-website`，`main` 推上去 Vercel 自动上线），有自己的 `AGENTS.md`、`ROADMAP.md`、`DEPLOY.md`，碰之前先读。它的规矩：Agent 不审查、不代拟网站内容；push、PR、部署要站主授权；方案获批不等于开工。
- 内容只在「呦呦漫游」（`site/`）里编辑和发布：只在网站文件夹里，不进 `pomelo.db`、备份、导出、撤销、搜索；内容格式只在 `site/` 的 Rust 里写一份，网站改了格式我会说。
- **真的网站文件夹只读**：只读文件、只用只读的 git 命令（status、log、diff、show），提交、推送、拉取、切分支由我做。开发和测试用副本加本机的假远端（`site::mode`），绝不往真的 GitHub 推。改网站项目里的代码和文档要我说开工，只改文件、不碰 git。

## Seeker

- `~/Documents/Seeker` 是我的求职资料（不是 Git 仓库，有自己的 `AGENTS.md`）。Pomelo 里的「Seeker」是柚子的助手（数据在 `youzi.db`，资料以 Pomelo 为准；需求见 `requirements.md`「Seeker」「Rater」）。
- 真文件夹的边界：`AGENTS.md` 只读；`knowledge/` 只读文件夹、文件名和 `招聘索引.md`；`material/` 只看文件名（Pomelo 也只列文件名，不读内容、不发给模型；只有 DevJanitor 的 Agent 能把文件名和大小发给模型）；`cv/` 能看文件名，两份原模板 `cv/cn/seek-it-cn-template-v1.docx`、`cv/en/seek-it-keyreply-resume-v5.docx` 只读（复制到临时文件夹里测；`cv/cn/seek-it-cn-template-v2.docx` 是内置中文模板导出的样例，编的林小柚），别的简历要读内容先问我；原件不改、不移、不删。
- 开发和测试只用临时目录里编的资料（照 `seeker/tests.rs`）；开发版打不开真的 Seeker 文件夹（Rust 里拦着）。简历、经历、证书这些个人资料不进 Git，文档和问卷里的例子也用编的。

## 柚子（AI）

- **名字**：AI 叫柚子（顶栏的柚子），代码、文件名、偏好键里叫 youzi；钥匙串的服务名「Pomelo OS Agent」不能改（改了存过的 Key 全读不到）；Old Timer 里聊天的是另一个人设，叫小拾。
- 数据只在 `youzi.db`（自己的升级脚本和版本号），不为柚子给 `pomelo.db` 加 migration。
- **锁不能套着用**：`with_db` / `youzi.with_db` 的闭包里不能再调用会自己上同一把锁的函数（比如 `youzi.selection()`），会卡死整个 App。页面反复调用、可能慢的命令写成 `async`（不写的在主线程上跑，会挡住键盘和触控板的事件）。
- **运行方式**由 `youzi::mode` 决定：只有正式数据目录的打包版（或 `POMELO_REAL_MODELS=1`）是真实的；开发版、测试、设了 `POMELO_DATA_DIR` 的打包版都是模拟（模拟模型、假的流式服务、Key 在内存，不调 `llmctl.sh`、不连 8080 / 8081 / 8082 和云端、不碰钥匙串）。
- **不为本地模型让步**：做功能不用考虑本地模型的性能和上下文；我说要适配才适配。
- **测试模型**：平时只用模拟的。要真模型时一律用云端，照 `docs/testing.md`「真模型和真接口」，用之前先问我；Key 在 `POMELO_CLOUD_KEY_FILE` 指的文件里，测试代码从不读钥匙串。本地模型只在我特别要求时测。
- **本地模型**：`~/Documents/LLM` 由你维护（部署、整理脚本和清单、保持轻量）。改之前说清楚，下载或删除模型文件先问我；只用它自己的文件夹和 launchd 标签，改完在它的 `INSTALLATION.md` 里写现状。聊天模型上下文：14B（及看不出大小的）最多 16K，9B 这样更小的最多 32K（`models.json`、`llmctl.sh`、Pomelo 都拦）。
- **签名**：打包用钥匙串里的自签名证书 `Pomelo Self-Signed`（`tauri.conf.json`），更新后钥匙串里的 Key 才还能读；不要改回临时签名。
- **永远不做成 AI 工具**：启停模型、芝麻开门、导入导出、备份恢复、写文件（保存为 Word、改文件都由我点；改文件不留备份），也没有删 Pomelo 数据的工具。唯一能动磁盘上文件的是 DevJanitor（见下）。云端 Key 和插件库的 Key（Tavily、TMDb、Mathpix）只在钥匙串，不进数据库、日志、导出。
- **查数据**：只调用各模块的只读函数；能不能读、能不能发给云端都在 Rust 里判断（芝麻开门、隐藏的模块挡掉），界面只传当前页面和记录编号。合计和统计只写一份：查询和页面共用 Rust 函数。
- **思考**：每次调用模型都传要不要思考（`Options.thinking`），模型做不做得到在 `provider.rs` 里按 `cloud::Think` 和模型清单处理。强度只在 `provider.rs` 的 `level_for` 决定发什么（模型清单里填 `efforts`、`default_effort`），新的调用点不用管；要让我选强度的地方用 `cloud::Level`（或存「开关 + 强度」）和 `Thinking.tsx` 的 `LevelPicker`；连续调用很多次的中间步骤不传强度；直接连接口的（`pub2md/search.rs`）照 `CloudModel::effort`。开思考时用 `Route::with_thinking` / `context::think_room` 多留回复长度。
- **问我（ASK）**：`ask.rs`。模型停在 `Gate` 上等我回答，只给云端模型的对话；不能等人的后台任务不提供它。
- **写数据**：模型只能提议，Rust 用模块自己的校验试写（在撤回的事务里）后存成卡片，我确认后才写，整张卡片一步 `with_undo`。没确认的卡片，模型用同一个工具加 `replace`（编号）换掉其中一条（`act/card.rs`）。
- **插件库**（`youzi/skills/`）：技能是 Rust 写的固定功能。发给 Wikidata / Open Library 的请求带我填的联系邮箱（`youzi.db` 偏好 `skills.contact`，不写进代码）。
- **助手**（`assistants/`）：名字由我来定，新加的先问我（现在有 Translator、Pub2MD、Seeker、Rater、Importer、DevJanitor、Atlas）。
- **定时任务、今日简报、年度总结**：Pomelo 的数据只读，不给写数据的工具；定时任务能联网搜索、用文件工具箱（只提议，卡片留在那次的对话里）；简报和年度总结不给查数据的工具（Rust 先用 `data.rs` 读好）。开机自动启动只有正式数据目录的打包版会登记，测试不碰真实的登录项。
- 模拟模型和 `mockIpc.ts` 都要能模拟查数据、提议写数据、定时任务、简报和年度总结；按意思搜用模拟的向量和重排，联网搜索、书单 / 影单检索用模拟的结果。

## DevJanitor

- 柚子的电脑管家助手（需求见 `requirements.md`「DevJanitor」），代码、模块键里叫 janitor。
- **运行方式**：只有正式数据目录的打包版扫真磁盘、真的删、真的跑命令。开发版、测试、设了 `POMELO_DATA_DIR` 的打包版扫临时目录里编的「假 Mac」：删除和废纸篓都在假 Mac 里，命令不真跑（返回编的结果），开发环境也是编的。要看真磁盘上的速度和准确性先问我，只读地扫（名字和大小），跳过 Pomelo 的数据目录和 Seeker 文件夹，不删、不跑命令。
- **扫描结果**在 `janitor.db`（像 `search.db`，改结构把它的版本加一）。设置、Agent 加的一键清理规则、Agent 的对话在 `youzi.db`。
- **Agent 只能提议**：移到废纸篓、跑命令、加进一键清理三种卡片（备份另有在硬盘上改名或挪一种，`backup/cards.rs`），我点了才执行；含 sudo 的只给命令，不出「执行」。唯一的例外是 `janitor/readonly.rs` 清单里的只读命令（du、ls、brew list 这类，不接管道、重定向、变量）：Agent 自己跑、直接看输出，不出卡片；加命令要确认它真的只读。清理、移到废纸篓、跑命令不进撤销；加、删一键清理的规则用 `with_youzi_undo`。能自己决定联网搜索。
- **备份**（子页，1.1.0）：要备份的文件任何程序和模型都不打开、不预览、不算校验码（复制和上传本身除外），只读名字和元信息。名字叫 VAULT 的（`~/Downloads/VAULT.sparsebundle`、移动硬盘上的 `VAULT/`）不列、不进、不备份、名字也不发给模型，Agent 查文件夹和只读命令也拦（写进 `rules.rs`）；你自己也绝不读。硬盘上只加不删，改名走卡片；PikPak 只动它那个文件夹，多出来的、被换掉的挪进那里的 ARCHIVE，我点了才传。开发和测试用假 Mac 里编的「下载」和移动硬盘、rclone 拿本机目录当假云端；真硬盘、真云端先问我，真硬盘只读元信息。真实的文件夹名不写进文档、测试和提交，例子用编的。
- **受保护的地方**只在 `janitor/rules.rs` 写一份，三种卡片、一键清理、Agent 加的规则都查：Pomelo 的数据目录、钥匙串、`~/Documents/LLM`、Seeker 文件夹、个人网站文件夹里除了依赖和构建产物的东西、系统本身。能看、名字能发给模型；Agent 的卡片碰到它们标「受保护」、执行前再确认一次；一键清理永远不进。

## 界面规则

做界面的规矩只写在这里；视觉、共用的摆法和控件表见 `docs/design.md`。

- 不写使用提示，列表行只放最必要的信息；只有一种东西的列表不写小标题。
- 先用现成的组件（控件表、`PageHeader`、`SidePanel`、`.side-row` + `<Picked />`、`charts.tsx`、手画的 `TextInput` `MdField` `Dropdown` `PickField` `Button`），不用细边框的原生输入框，不自己拼 `pm-*` 类名；我能挑图标的地方用图标库（`IconDialog` / `IconField` / `StoredIcon`，存成 `i:<键>`）；小按钮的名字用 `pm-tip-host` + `<Tip>`，纸签只写动作（「删除」，不写「删除「某某」」：记录名放 `IconButton` 的 `label`、纸签写 `tip`）；浮在字或按钮旁的卡用 `.pm-float` + `src/shared/fit.ts`（量了再摆，不出窗口；带标题的能拖，`useDragFit`）；弹窗用 `Modal`，自己写 `<dialog>` 的用 `shared/dialog.ts` 的 `showDialog` 打开。
- 颜色只用变量（图表只用 `--chart-*`；新模块的颜色 `--module-<键>` 在那边的 `schemes/pomelo/tokens/targets.json` 定、同步过来，这边在 `app/modules.ts` 写 `color: "var(--module-<键>)"`）；字体只用 `font-sans`（不直接写 `Hannotate SC`；柚子消息正文和见字如面的正文用苹方 `font-reading`，是仅有的例外）；便签（`.pm-note`）上用便签的墨色。
- 白卡照写 `rounded-card border bg-card`，在纸上自己变成手画卡。手画的边画在字下面一层：会滚、会裁的盒子外面一层画边、里面一层滚，能滚的区域用 `-m-1 … p-1`。拖动排序用鼠标事件（`src/shared/sortable`），不用浏览器自带的拖放。
- 动效只加减类名：先在 `ds.css` 里找零件，没有的回 visual-atelier 加，这边不写关键帧。
- 每一页、桌面和每个浮层都包 `shared/Oops.tsx`（浮层用 `look="layer"` 加 `onClose`，照 `App.tsx` 里贴纸簿的写法）；hook 不写在提前返回的后面；有焦点的输入框不能在窗口外面（WebKit 会把整个窗口的内容挪上去）：从窗口外滑进来的东西进了窗口再给焦点，滑走前先放开。页面用 `usePageContext` 告诉柚子小窗现在在哪。
- 窗口是 WebKit：不放实时的 SVG 滤镜，整页大小的层不改 `z-index`、不反复新建；改了外框、纸、手画零件或动效，在 Safari 里量一遍流畅度。
- 外框（顶栏、桌面和纸、前进后退、贴纸簿、柚子瓣、柚子）照 `requirements.md`「界面外框」和 `design.md`「外框」；各模块自己的布局不动。面板里柚子的操作（贴 JD、拆成子任务）放在那一栏标题的右边：`Field` 的 `action` 加 `Assist.tsx` 的 `AssistAction`。
- 样式只管自己：模块的规则写在自己的盒子下面或带自己的前缀，不写单个通用类名或只有元素打头的规则（`pnpm check` 的 css-lint 拦）；会滚的区域里只为裁切的盒子（截断的字、折起来的卡片、左滑才露出的按钮）用 `overflow: clip`，不用 `hidden`（WebKit 里会接走鼠标滚轮，ui-lint 的 wheel-trap 查；`index.css` 已把 `truncate`、`line-clamp-*` 改成 `clip`）。
- 改了界面跑 `node scripts/ui-lint.mjs`（白板、直线框、被切的边、字出框都要清零）和 `node scripts/hooks-lint.mjs`。改了写字的框（`shared/editor/`、`bear.ts` 和它们的样式）再跑 `node scripts/editor-lab.mjs --safari`（WebKit）。

## 文档规则

- **给我看的**：`README.md`、`CLAUDE.md`、`AGENTS.md`、`HANDOFF.md`、`docs/`。越短越好，通俗、全中文，只写现状、规则和决定，不写变更历史、不写只为旧数据的说明；各页的具体样子（布局、宽度、按钮上的字）以代码为准，不写进文档。
- **不给我看的**：代码、注释、脚本、提交信息，按 Agent 效率最高的方式写。
- **一件事只写在一处**，别处要提只写「见 `某文档`「某节」」：`README.md` 是什么、怎么用、数据在哪；`CLAUDE.md` 给 Agent 的规则和代码地图；`HANDOFF.md` 现状、没做完的、等我做的（每轮更新）；`roadmap.md` 以后做什么、做完的版本（一个版本或 Phase 做完更新）；`requirements.md` 功能做什么、我的决定；`architecture.md` 做法和为什么；`schema.md` 表、字段、数据文件格式和版本记录；`design.md` 视觉、共用的摆法和控件；`testing.md` 怎么测。哪样变了就改哪份。

## 代码地图

| 位置 | 内容 |
|---|---|
| `src-tauri/src/modules/` | 每个模块一个文件（`todo` `finance` `jobs` `notes` `articles` `days` `vault` `books` `films` `games` `drawing` `packing` `bookmarks`），开头是规则摘要；`common.rs` 校验、日期、排序、链接、`Tx`；`links.rs` 柚子链接（`[[…]]` 的写法、认名字、反向链接、改名跟着改；能链到新的一类东西在 `KINDS` 加一行）；`tags.rs` 正文里的 #标签（念念不忘、见字如面共用）；`kinds.rs` 可自定义的类型；`attachments.rs`；`custom.rs` · `lists.rs` · `progress.rs` 五花八门和循序渐进；`articles/` 导入（Bear、Markdown / TextBundle）、导出、`docx.rs`；`finance/import.rs` 导入账单 |
| `src-tauri/src/search/` | `query.rs` 唯一的搜索入口（按字 / 按意思 / 合并重排）、`mod.rs` ⌘K、`store.rs` search.db、`sources.rs` 每类记录进索引的字段、`text.rs` 分词（改了把 `RULES` 加一）、`vectors.rs`、`indexer.rs` 后台同步 |
| `src-tauri/src/youzi/` 模型 | `mod.rs` 运行方式、状态、后台循环；`runtime.rs` 本地模型和向量、重排服务；`catalog.rs` 模型清单；`cloud.rs` 云端模型、`Think`、Key；`provider.rs` 流式连接（OpenAI 兼容、Anthropic、模拟；本地模型排队）；`embed.rs` · `rerank.rs` |
| `src-tauri/src/youzi/` 对话 | `store.rs` youzi.db 和升级脚本；`chat.rs` 对话和生成循环；`context.rs` 上下文预算；`library.rs` 对话列表；`instructions.rs` · `templates.rs` · `memory.rs`；`files.rs` · `pdf.rs` · `office.rs` · `reading.rs` · `index.rs` 附件和资料；`knowledge.rs` 知识库；`scans.rs` 知识库扫描件的本机识别（ocr.db、后台队列）；`work.rs` · `word.rs` 保存为 Word、改文件、照模板写简历；`excel.rs` Excel 工具箱；`find.rs` · `export.rs` · `transfer.rs` · `storage.rs` |
| `src-tauri/src/youzi/` 能力 | `data.rs` 查 Pomelo 数据；`cite.rs` 出处；`ask.rs` 问我；`act/` 写数据的工具和确认卡片；`assist.rs` 润色、补全、一句话填好 / 改好；`skills/` 插件库（`mod.rs` 每个技能的流程图 `Flow`，照代码真实的判断和循环写）；`schedule.rs` 定时任务；`brief.rs` 今日简报；`yearly.rs` 年度总结（流程图 `FLOW`）；`atlas.rs` Atlas 的「呈现结果」、历史、首页信封；`made.rs` 自建模块的一幕 |
| `src-tauri/src/youzi/assistants/` | `seeker.rs` + `seeker/`（内置简历模板在 `src-tauri/templates/`）；`rater.rs` + `rater/`；`translate.rs`；`importer.rs`；`janitor/`（`rules.rs` 类别、受保护的地方和 VAULT、`backup/` 备份（`diff.rs` 对比、`copy.rs` 复制和 ARCHIVE 里的旧版、`volumes.rs` 认移动硬盘、`hints.rs` 给柚子的线索、`cards.rs` 柚子的卡片、`look.rs` Agent 看备份和自动看一眼、`rclone.rs` rclone（加密配置、同步、进度）、`pikpak.rs` PikPak 对比、同步、记录）、`scan.rs`、`store.rs` janitor.db、`system.rs`、`env.rs` 开发环境、`agent.rs`、`readonly.rs`、`fake.rs` 假 Mac、`mod.rs` 运行方式和清理）；`site.rs` 呦呦漫游的起草；`pub2md/`（流程在 `job.rs`） |
| `src-tauri/src/site/` | 呦呦漫游：`mod.rs` 运行方式、副本、命令；`tools.rs` git / gh / Node；`server.rs` 本地服务；`changes.rs` · `publish.rs`；`yaml.rs`（照 js-yaml 写，`yaml_cases.rs` 对照）· `doc.rs` 内容文件；各页 `wander.rs` `now.rs` `about.rs` `career.rs`；`registry.rs` · `images.rs` · `terms.rs` |
| `src-tauri/src/` 其他 | `home.rs` 首页；`calendar.rs`；`health.rs` 体检；`dock.rs` 贴纸簿的实时小字；`icons.rs` 网站图标；`canvas.rs` 有板有眼（启停、询问 visual-atelier 的 bench，不存数据）；`tray.rs` 菜单栏的柚子；`menu.rs` Mac 菜单；`capture.rs` 速记小窗；`print.rs` 见字如面导出 PDF / Word 的隐藏窗口；`autostart.rs`；`notify.rs` 系统通知；`undo.rs` 撤销（SQLite session，分两个库）；`secure.rs` 钥匙串、Touch ID、剪贴板；`transfer.rs` · `backup.rs`；`db.rs` · `migrations/`；`paths.rs`；`settings.rs` |
| `src/app/` | `App.tsx` 外壳、⌘Z / ⌘K / ⌘J、例行检查；`TopBar.tsx`；`tabs.tsx` 开着的页和前进后退、`subpage.ts` 子页；`Desk.tsx` 桌面上的纸和「常用」；`StickerBook.tsx` 贴纸簿；`Bloom.tsx` 柚子瓣；`dock.ts` 分类、置顶、隐藏（代码里还叫模块坞）；`LookPicker.tsx` · `moduleLooks.ts` · `looks.ts` 模块的图标和颜色；`Pomelo.tsx` 柚子的表情；`modules.ts`（模块、助手、子页）；`Search.tsx`；`pageContext.ts` |
| `src/home/` · `src/calendar/` | 首页（`HomePage.tsx`、`Desk.tsx` 拖桌面和便签、`notes.tsx`、`BriefCard.tsx`、`YearCard.tsx` 新年的信封）；日历页 |
| `src/print/` · `src/capture/` | Vite 的另两个入口：`print.html` 见字如面导出用的打印页（`layout.ts` 排成 A4 页、拆成 Word 的块；`print.html?mock&id=…` 在浏览器里看）；`capture.html` 速记小窗（`main.tsx` 和主窗口一样要 `startHand()`） |
| `src/modules/<模块>/` | 各模块页面。`articles/`：`Editor.tsx` 写字的框和格式条、`bear.ts` Bear 的 Markdown 怎么画、`render.ts` 公式代码流程图（编辑器和打印页共用）、`swipe.ts` 双指滑动（改了跑 `scripts/swipe-sim.mjs`）、`Shelf.tsx` 书架；标签树用 `notes/TagTree.tsx`。书影游共用 `src/shared/collection/`；`custom/` 五花八门、循序渐进；`site/` 呦呦漫游；`atlas/`（`Journey.tsx` 翻开的小报，`designs.ts` 按名字和版本找设计快照） |
| `src/modules/youzi/` | `YouziPage.tsx`；`ConversationList.tsx`、`BigPomelo.tsx`；`Transcript.tsx` · `markdown.ts`；`useChat.tsx`；`SideChat.tsx` 右侧小窗；`AskCard.tsx`；`Composer.tsx`；`Assist.tsx` 面板里的帮手；`Thinking.tsx` 思考和强度；`assistants/` 各助手的页面（`seekerJobs.tsx` 让模型调用离开页面也跑完；`translator.tsx` 和速记小窗共用） |
| `src/settings/` · `src/shared/` | 设置页（五张纸签）；共用组件：`links/` 柚子链接（改了能被链到的东西调 `links.ts` 的 `linksChanged`）、`editor/`（`LiveText` 底子、`LiteEditor` 念念不忘的格式（新模块要写字就用它）、`MdField` 写提示词的框、`MdMenu.tsx` 右键的格式）、`iconLibrary.tsx`、`tips.ts` 落签、`hand.ts` 手画零件、`ui.tsx` 基本控件、`Oops`、`InlineName` 就地改名（改名都用它，不弹窗）、`Dropdown` `PickField` `DateInput` `panel` `Modal` `sortable` `charts` 等 |
| `src/dev/` | `mockIpc.ts` 浏览器测试用的模拟后端（新命令要加进来；各模块的 `*Mock.ts` 分开放，`backupMock` 是备份的，`janitorMock` 由 Rust 扫假 Mac 生成，`atlasMock` 由 `scripts/atlas-mock.mjs` 生成）；`perf.ts` 流畅度自测、`edtest.ts` 写字的框自测、`drive.ts` 在 Safari 里遥控（见 `docs/testing.md`） |
| `scripts/` · `prototype/` | `install-app.sh`、`clean-build.sh`、`perf-listen.mjs`、`drive.mjs`、`ui-lint.mjs`、`hooks-lint.mjs`、`css-lint.mjs`、`editor-lab.mjs`、`swipe-sim.mjs`、`atlas-mock.mjs`、`resume-templates.ts`（生成内置简历模板）；界面原型 `prototype/ui-rN/`（不进 Git，现在是 `ui-r4`） |

## 新增一个模块

1. 新增 migration，加进 `db.rs` 的 `MIGRATIONS`
2. 写 `src-tauri/src/modules/xxx.rs`（参考 `jobs.rs`），在 `mod.rs`、`lib.rs` 注册；写导出再导入的往返测试
3. 需要「类型」：在 `kinds.rs` 的 `TABLES` 加一行（列名必须叫 `kind_id`），默认值写进 migration
4. `transfer.rs` 加一个 `Payload`（含 `kind_modules`），格式版本加一；`search/sources.rs` 加要搜的字段，页面用 `useOpenRequest` 打开搜到的记录；要上首页的在 `home.rs` 加一块；柚子要能查的在 `youzi/data.rs` 加查询
5. 前端：`src/modules/xxx/`，在 `app/modules.ts`、`main.tsx` 注册，`app/dock.ts` 的 `DEFAULT` 里定它默认在贴纸簿哪个分类；有子页的在 `modules.ts` 写 `subs`、页面用 `useSubPage`；模块颜色和贴纸回 visual-atelier 加（见「visual-atelier」）；命令和示例数据加进 `mockIpc.ts`；页面用 `usePageContext` 告诉小窗在哪、打开了哪条
6. 更新文档

五花八门、循序渐进建的模块不走这一步。

## 常用命令和验证

| 命令 | 作用 |
|---|---|
| `pnpm check` | 样式不串 + 类型检查 + 双指滑动的模拟 + 写字的框的测试台 + Rust 测试，每次改完都要跑 |
| `pnpm app:dev` · `pnpm app:build` | 运行开发版 · 只打包不安装 |
| `pnpm app:install` | 打包并安装到「应用程序」（什么时候装见「正式版」） |
| `pnpm clean:build` | 清编译缓存 `src-tauri/target`（超过 20 GB 时各命令和开会话的 hook 会自动清；别改回带调试信息的编译设置） |

- 怎么测见 `docs/testing.md`：界面用 Bash 后台跑 `pnpm dev`、再开 `ui-mock` 的 `http://localhost:1420/?mock`；`?mock` 不走 Rust，新加或改了 Rust 命令的页面还要在打包版里用临时数据目录点一次（不用 osascript 碰 System Events）。

## 环境

- macOS，Apple M3，18 GB 内存。Node 24、pnpm（`~/Library/pnpm/bin`）、Rust（`~/.cargo/bin`），非交互终端里要先加进 PATH。
- 辅助脚本用 TypeScript（Node），不引入 Python。
