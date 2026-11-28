# 技术架构

> 怎么做的、为什么这么做：选型、分层、数据存放、开发版和正式版、升级、备份、附件、撤销、安全和跨模块的机制。不写功能（`requirements.md`）、表和字段（`schema.md`）、Agent 的规则（`CLAUDE.md`）；单个文件的细则在那个文件开头。

## 技术选型

| 部分 | 选择 |
|---|---|
| 桌面应用 | Tauri 2 |
| 界面 | React + TypeScript + Vite，Tailwind CSS；少量自己写的组件，不用组件库 |
| 数据层 | Rust + SQLite |
| 柚子（AI） | Rust 用 reqwest 连模型、收流式回复；消息排版用 markdown-it、highlight.js、KaTeX |
| 见字如面 | 自己的 `LiveText` 加 Bear 的 Flavor；highlight.js、KaTeX（和柚子共用）、Mermaid（真有流程图时才加载）；从 Bear 搬家调只读的 `bearcli`；PDF 用系统的 WebKit 排版、engine-pdf 合成，Word 自己写 XML（zip），不加依赖 |
| 读写文件 | 共用引擎 engine-pdf（PDF：PDFKit 取字和画页、PDFium 补连字，静态链进 App，PDFium 的许可证随 App 带在 `THIRD_PARTY.txt`）、`textutil`（读 Word）、`sips`（缩图片）；`zip` + `quick-xml`（PowerPoint、改写 Word），`calamine`（表格），`dom_smoothie`（网页正文） |
| 系统通知 | UserNotifications（`objc2-user-notifications`） |
| 辅助脚本 | TypeScript（Node），不用 Python |

## 分层

```
界面（React） ──┐
               ├── 调用命令 ──▶ Rust 命令层 ──▶ SQLite（pomelo.db · youzi.db · search.db · janitor.db）
柚子的 Agent ───┘              业务规则、校验都在这里
                               │
                               └─ 柚子服务 ──▶ 本地模型 8080 · 向量 8081 · 重排 8082 · 六家云端
```

- 界面只调 Rust 命令（如 `books_list`）；柚子的工具也是 Rust 命令（只读查询、「提议」），碰不到数据库。
- 用模型都经过「柚子服务」（聊天、定时任务、简报、润色补全、翻译）；本机别的程序用本地模型直接连 8080。
- 哪些命令写成 `async` 见 `CLAUDE.md`「柚子（AI）」；`async` 的在后台线程上跑、先后不定，要按顺序的在页面里排队发（如见字如面的保存）。

## 数据存放在哪里

| 位置 | 放什么 | 备份 |
|---|---|---|
| `/Applications/Pomelo OS.app` | App 本体 | 不用 |
| `~/Library/Application Support/Pomelo OS/` | 真实数据：`pomelo.db`（生活数据和设置）、`youzi.db`（柚子）、`backups/` | **要** |
| 同上的 `attachments/` | 附件（原文件的副本） | 不进每日备份，交给 Time Machine 这类工具 |
| 同上的 `youzi-files/` | 柚子的附件和项目资料（另存一份，不和上一行共用） | 不用 |
| 同上的 `search.db`、`janitor.db` | 搜索索引、DevJanitor 上次扫描的结果 | 不备份、不导出，删了重建 |
| 同上的 `icons/` | 一网打尽的网站图标（一个网站一个文件） | 不备份、不导出，删了重新取 |
| 数据目录的 `rclone/` | 备份用的 rclone 配置（PikPak 的登录），rclone 自己加密，密码只在钥匙串 | 不备份，丢了重新登录 |
| `~/Library/Application Support/Pomelo OS Dev/` | 开发用的测试数据；`janitor-sandbox/` 是 DevJanitor 的「假 Mac」，`janitor-volumes/` 是备份用的假移动硬盘，`janitor-cloud/` 是假的 PikPak | 不用 |
| macOS 钥匙串 | 「Pomelo OS Agent」（云端、插件库、Pub2MD 的 Key 合成一项）、「Pomelo OS 芝麻开门」（Touch ID 用的钥匙） | 不进备份 |
| `~/Documents/LLM` | 本地模型、模型清单、启停脚本 | 由 Claude 维护；Pomelo 只读清单、调脚本 |
| `~/Documents/GitHub/personal-website` | 个人网站（呦呦漫游编辑、发布它） | 独立项目，自己的 git 和 GitHub 仓库 |

数据库别放进 iCloud、Dropbox 这类同步文件夹（可能弄坏）；异地保存就复制**备份文件**。

## 开发版和正式版

- 同一份代码，只差数据目录：正式版是「应用程序」里的 App（真实数据，那里只留一个 Pomelo OS），开发版从代码运行（测试数据，随便清空）。`pnpm app:install` 更新正式版，新版第一次打开先自动备份再升级。
- **签名**：固定的自签名证书「Pomelo Self-Signed」，系统认得是同一个 App，更新后钥匙串里的 Key 还能读。自签名没有团队编号，钥匙串只按版本认，所以每次更新要再授权：打开新版时弹一次（Key 合在一项里），芝麻开门第一次用 Touch ID 前弹一次；输登录密码、点「始终允许」。完全不弹得换苹果的开发证书（要 Xcode 或付费账号），定了不换。证书丢了就重建：钥匙串访问 → 证书助理 → 创建证书，名称「Pomelo Self-Signed」、身份类型「自签名根证书」、证书类型「代码签名」（换了证书，钥匙串里存过的 Key 要重新授权）。
- **真的和模拟的**：只有正式数据目录的打包版连真模型、扫真磁盘、用真的网站文件夹、取网站图标、弹系统通知、登记开机启动；开发版、测试、设了 `POMELO_DATA_DIR` 的打包版都是模拟的。单独打开真的：`POMELO_REAL_MODELS=1`（先问我）、`POMELO_REAL_SITE=1`、`POMELO_REAL_NOTIFY=1`。
- **编译缓存**：`src-tauri/target` 每换一种编译方式多一份、只增不减，所以开发编译时依赖不带调试信息、自己的代码只带行号（完整编译测试一次约 3 GB），超过 20 GB 整个清掉（`scripts/clean-build.sh`，从头编约一分半）。

## 数据库升级

- 只新增编号脚本（`src-tauri/migrations/`，规则见 `CLAUDE.md`「数据规则」）；`youzi.db` 有自己的一串和版本号。
- 启动时要升级就**先自动备份**再逐个执行；数据库比 App 新（装回了旧版）时拒绝打开。
- `search.db`、`janitor.db` 只有版本号、不升级：对不上就删了重建。

## 备份与导入导出

| | 快照备份 | 数据文件（JSON） |
|---|---|---|
| 是什么 | 整个数据库的副本，在 `backups/` | 所有模块的数据，人能看懂（格式见 `schema.md`） |
| 用途 | 出问题时恢复 | 长期归档、换工具、从 Excel 导入 |
| 什么时候做 | 每天第一次打开、升级前、覆盖导入前、恢复前自动做；也能「立即备份」 | 「设置 → 导出…」 |
| 怎么用回来 | 设置里的备份列表，「恢复」 | 设置里「导入」 |

- **留多少**：每日备份 `pomelo.db` 留 30 份、`youzi.db` 7 份，更早的每月留最后一份；两个库分开显示、分开恢复。升级前、导入前、恢复前的各留 3 份（做每日备份时清）；手动的不自动删。文件名写着库、时间和原因（如 `pomelo-20260929-093000-daily.db`，见 `backup.rs` 开头）。
- **恢复**把当前数据整个换成那份（先备份当前的）；老版本的恢复后自动升级，损坏的或比 App 新的拒绝。**导入**的规则见 `schema.md`「数据文件（导出 / 导入）」，覆盖前先确认、备份。

## 附件

- **存**：Rust 把文件复制到 `attachments/`（随机名、只有我的账户能读写），按 SHA-256 同样的内容只存一份；界面只拿到编号、名字、大小。
- **看**：图片走 `pomelo-file://localhost/<编号>`（Rust 只按编号给已存下的图片）；其他文件交给 macOS 用默认程序打开。
- **拖进来、粘贴**：交给落点所在的那块（`shared/attachments/target.ts` 开头）；粘贴的截图没有路径，界面把内容直接交给 Rust。
- **删**：删记录不删副本（能撤销），没人用的在「设置 → 数据 → 附件」清理。

## 芝麻开门的安全

- **加密**：设主密码时随机生成「密码库钥匙」，每条密码和分类名用它加密（XChaCha20-Poly1305）后才写库，明文只有顺序、分类编号、归档。主密码经 Argon2id（256 MB、3 轮，约 0.3 秒）算出另一把钥匙，锁住密码库钥匙存着；主密码本身哪里都不存。
- **打开**：输主密码，或用 Touch ID 从钥匙串取出密码库钥匙；规则（3 次失败、3 天、更新后）在 Rust 里判断。打开后钥匙只在内存，5 分钟没用就清掉（按真实时间，睡眠也算）。
- **谁读得到**：数据库、备份、导出里没有明文，读文件的程序（包括 AI 编程助手）只看到乱码；钥匙串那份别的程序要读时系统要登录密码。复制密码由 Rust 放进剪贴板（标成「隐藏」），30 秒后清掉；列表从不带密码。
- **还剩的口子**：① 钥匙串里 Touch ID 那份只靠电脑登录密码挡着；② 拿到数据库或备份的人能离线试主密码（每次约 0.3 秒），越长越难；③ 复制后 30 秒里别的程序读得到剪贴板，「显示」时能截屏的程序看得到；④ 正式版没开「强化运行时」，以我的身份运行的恶意程序理论上能注入 App。「只用主密码」开关、强化运行时定了不加。主密码忘了就打不开，没有找回办法。
- **见字如面的加密**用同一把钥匙（`vault::seal_article`）：芝麻开门开着才能读写，不另存钥匙、不另设密码。标题和标签是明文（列表和标签树要用），正文里的图片是普通附件。

## 撤销

- 改数据的命令执行时用 SQLite 的「会话」记下改了哪些行（连同一起删的子项）；⌘Z 反着应用，⌘⇧Z 再正着应用。`pomelo.db`、`youzi.db` 分开记，最多 100 步，只在内存。
- 不进撤销的：设置、发消息、生成回复、建索引、删副本、后台自己跑的（例行检查、定时任务、简报）；可撤销的编辑不碰它们记的东西（否则 ⌘Z 被挡住）。
- 数据在这之间被改过、对不上就拒绝撤销并清空记录；导入和恢复备份也清空。

## 拖拽排序

拖完把这几条的新顺序交给 Rust；可以只是一组的一部分（没做完的任务、筛选后的课节），在它们原来的位置之间互换，看不到的不动。Rust 查过都属于这一组再重写 `sort_order`。拖动用鼠标事件自己做（macOS 的网页控件对原生拖放支持不好）。

树（一网打尽，念念不忘和见字如面的标签）一次拖动交给 Rust「放进哪一层、放在哪一条前面」（一网打尽是 `bookmarks_place`；标签换了一层就是改名），Rust 查过再重新编号，一步 ⌘Z。落点怎么算见 `BookmarksPage.tsx`、`TagTree.tsx` 开头。

## 写字的输入框

`src/shared/editor/` 的 `LiveText` 是共用的底子：只收纯文字的可编辑框，文字是唯一的数据，每改一次按行重画（只动变了的行）、按位置放回光标；输入法拼字时整个框归它，不重画、不读回。各种写法（Flavor）只管怎么画：`lite.ts` 念念不忘的格式（`LiteEditor`）、`markdown.ts`（呦呦漫游的正文、写提示词的 `MdField`）、见字如面的 `bear.ts`。细则在 `LiveText.tsx`、`editable.ts` 和各 Flavor 开头；右键的那条纸见 `MdMenu.tsx`，字里的图标见 `inlineIcon.ts`。

- **见字如面**（`bear.ts`、`Editor.tsx`）：记号只在光标所在处露出，别处画成「预览」（不算进文字）；代码、公式、流程图按源文字缓存（`render.ts`）。专心写时正文的节点整个挪到 `<body>` 底下，编辑框不重建，光标和 ⌘Z 都在。
- **双指滑动**（`swipe.ts` 的 `Glide`）：滚轮事件不带「手指还在不在」，App 忙时还会晚到、几个并成一个，所以只有手真会做的才算新的一次（细则在 `swipe.ts` 开头）。
- **图标库**（`shared/iconLibrary.tsx`）：存成 `i:<键>`；Rust 只管长度（`common::clean_icon`），不认识的键只是不画。

## 界面外框

- **开着的页和前进后退**（`src/app/tabs.tsx`，只在界面里）：用路由地址记一个模块停在哪（子页是第二段，`subpage.ts`），开着的页存在设置 `ui.tabs`。贴纸簿、柚子瓣、全局搜索、顶栏走 `open`（开着的拿到最前，没开的新开），页面里自己的跳转由外框记下。
- **贴纸簿和柚子瓣**（`dock.ts`、`StickerBook.tsx`、`Bloom.tsx`；代码里叫 dock）：同一份设置（`schema.md`「状态、类型、设置」）的两种画法，都不进撤销；柚子瓣编辑时只更新内容、不重播动画。拖动用鼠标事件（`drag.tsx`）。实时小字是只读命令 `dock_live`（`dock.rs`）。
- **柚子的表情**（`Pomelo.tsx`）：今日简报的心情由 Rust 从模型写的最后一行取出、存进 `briefs.mood`；在写读 `youzi_status`；离开页面还在跑的活（发布、Pub2MD）用 `watchJob` 登记，外框每 2 秒问一次直到做完。
- **速记小窗**（`capture.rs`、`src/capture/`，细则在 `capture.rs` 开头）：同一个 App 的第二个窗口，第一次用才建、之后只藏起来（重建要半秒多）；⌥Space 用 Tauri 的全局快捷键插件（开发版也抢这个键）。和主窗口用事件来回通知。
- **Mac 菜单栏**（`menu.rs`）：自己建的菜单，代替 Tauri 默认的（它的 ⌘W 是关窗口）。「页签」菜单发 `app-menu` 给主窗口；⌘1～⌘9 的名字由页面发来（`app_menu_index`），Rust 重建菜单。

## 打开链接

带协议的（`https:`、`mailto:`、`bear:` 等）直接交给系统；`/…`、`~/…` 当本机文件；其他的补上 `https://`（`common::link_target`）。

## 模块之间

改好几张表的操作放在一个事务里（`common::Tx`，可嵌套：模块互相调用、导入时调各模块都不冲突），中途出错全不算。首页、搜索、柚子读各模块都走同一套：`home.rs`、`search/sources.rs`、`data.rs`。

- **一网打尽**（`modules/bookmarks.rs`）：文件夹和网址一张表，界面拿整张表自己搭树。网站图标（`icons.rs`）只从网站本身取：先读首页的 `<link rel="icon">`，再试 favicon 的常见位置，子域名都没有就试主站；取到发 `bookmark-icons` 让页面刷新，取不到的 7 天后再试。
- **五花八门、循序渐进**（`custom.rs`、`lists.rs`、`progress.rs`）：界面的模块列表 = 固定模块 + 从 Rust 读到的建出来的。五花八门每条的字段值存成一份 JSON，规则在 `lists.rs` 里查；排序、筛选只在页面上；选项和类型按在列表里的位置配颜色、不存（`shared/tags.tsx`）。柚子用 `pomelo_lists`、`pomelo_progress` 查，工具说明里不写模块名。
- **导入账单**：账单和行在 `modules/finance/import.rs`（`pomelo.db`，进撤销和导出）：每行留读到的原文和我核对后的草稿，确认后才写进 `fin_entries`。读账单的 Importer（`youzi/assistants/importer.rs`，细则在它开头）只用云端模型，分读、查重、再整理三步；写之前逐行比对，运行期间我动过的行不写。页面的任务在 `importJob.ts`，离开页面也跑完。
- **柚子链接**（`modules/links.rs`，规则只写在那里；界面 `src/shared/links/` 照着解析来画）：不另存表，链接就是正文里的 `[[…]]`，谁链到谁每次从正文现算。页面用 react-query 的 `["links"]` 缓存，能被链到的东西一改就作废（`linksChanged`，连念念不忘的列表一起：每条笔记带着它的柚子链接，缓存不会自己过期）。改名跟着改在同一个事务里（`follow_rename`）。
- **玩物有志**（`modules/games.rs`）：照摸鱼读读写，界面和书影共用 `shared/collection/`（`CollectionConfig` 开关多出来的几样）。
- **笔记转待办**：数据文件里任务用笔记的记录时间对上，所以导入时先导笔记再导待办。柚子「转为待办」勾选的整组交给 `todo_tasks_from_note`，照任务面板的规则逐张建、一步 ⌘Z（有一张不合规矩整步不做）。

## 搜索和索引

- **全局搜索**（⌘K，`search/mod.rs`）：Rust 在 SQLite 里按子串逐条比对每个模块要搜的字段（所以中英文都能找到词中间的字）。界面跳到那个模块，由模块页面打开那条（`src/shared/open.ts`）。
- **一个入口**：`search/query.rs`（细则在它开头），「意思相近」和柚子的搜索都调它；隐藏的模块、云端规则在这里挡，向量或重排服务用不了时照样按字返回。合并后只让重排模型有把握的排前面，其余保持合并后的顺序（短词它全给接近 0 分，全按它排会把对的挤下去）。
- **索引和原始数据分开**：`search.db` 只放算出来的；「哪些要建索引」这类选择留在 `youzi.db`。
- **后台更新**（`search/indexer.rs`）：自己的只读连接，每 2 秒看一眼两个库改过没有（SQLite 的 `data_version`，撤销、导入、恢复都算），有就按指纹比对，只写变了的、删掉没了的；不在命令里挂钩子。
- **向量什么时候算**：只在向量服务本来开着、或按意思搜 / 柚子检索要用时补算，不为改一条记录专门启动。重建（`search.db` 新建、换向量模型、换文字规则）例外：自动启动服务，后台算完，期间照常按字找。
- **速度**：向量压成 8 位整数载入内存逐条比（20 万段约 200 MB）；20 万条实测按字约 0.06 秒、按意思约 0.02 秒。再大就换近似检索，所以向量检索做成能换的部件（`search/vectors.rs`）。
- **全量读取**：年度总结这类要看全部数据的不经过检索，直接用各模块的只读函数读全。

## 呦呦漫游（个人网站）

- **内容只在网站文件夹里**（见 `CLAUDE.md`「个人网站」）：`site.folder` 记位置，不存副本；网站的 git 历史就是它的历史。格式照它的 `content.config.ts` 和现有文件。
- **运行方式**（`site/mod.rs`）：真的用真文件夹、真推 GitHub、用 gh 读 Vercel；其余用数据目录里的副本（`site-sandbox/website`，APFS 复制）和本机的假远端（`site-sandbox/remote.git`），上线结果是模拟的；远端不是本机文件夹时一律不提交、不推送。
- **外部程序**（`site/tools.rs`）：`/usr/bin/git`（推送靠钥匙串里已有的 git 凭据，要密码就报错、不卡住）；gh 只读上线结果；Node 按网站的 `.nvmrc` 在 nvm 里找。缺哪个只关掉用它的功能，写明原因。
- **本地服务**（`site/server.rs`）：网站的 `npm run dev` 在自己的进程组里跑，退出 Pomelo 时整组停掉；端口上已有别处启动的就直接用、不停它。预览是 Rust 开的 `site-preview` 窗口（不能调 Pomelo 的命令）。
- **发布**（`publish.rs`，后台任务，步骤见它开头）：fetch 只快进，确认框带着改动的指纹；`npm run qa` 过了、再确认没落后、指纹还对才提交并推送，然后读 Vercel 的上线状态。改动清单（`changes.rs`）按路径认出是哪类内容。
- **读写内容**（`yaml.rs`、`doc.rs` 和各页的文件）：读用 `yaml-rust2`（键保持顺序）；写是 js-yaml `dump` 的逐行移植（网站原有文件就是它写的），JSON 照 `JSON.stringify(…, null, 2)`。键原地改、不认识的原样留、没变的文件不写、写之前比对读进来时的指纹；有对照测试和 Node 跑的 js-yaml 逐字比对。
- **资源库**（`images.rs`、`registry.rs`）：内容只写引用；加文件都走 `site_files_add`，删之前照网站自己的规则扫引用。界面经 `pomelo-site://localhost/<网址>` 显示网站里的图片（只认 `public/` 下的）。

## 有板有眼

- **Pomelo 不存画布**：`canvas.rs` 只记文件夹（设置 `canvas.folder`，默认 `~/Documents/GitHub/visual-atelier`）和上次看的那块（`canvas.last`）。
- **画布服务**是 visual-atelier 的 bench（只听 127.0.0.1），Node 的找法、启停和呦呦漫游同一套（`site/tools.rs`、`site/server.rs`）。正式版用 bench 自己的 4321（碰上别处开的就直接用），开发版、测试、临时数据目录的打包版用 4331。接口照它的 `bench/README.md`「给别的程序用」，用到的地址和各 `api` 版本见 `canvas.rs` 开头；改名、删除照发，bench 不肯时把它的话原样给页面；删掉的进 visual-atelier 的回收处。批准、退回只在 bench 的界面里点。
- **页面**（`modules/canvas/`）：画布界面放在 frame 里，左边和 frame 用消息互相跟着（`CanvasPage.tsx` 开头）；同一个名字可以既是 Design System 又是 Motion Deck，所以左边按「类 + 名字」认。页面跨域取不到 bench 的 JSON，所以列表由 Rust 去问。

## Atlas

- **数据在 Rust**（`youzi/atlas.rs`，细则在它开头）：年度总结的信写完，Rust 先存一份当时能读的数字和记录的快照（「呈现结果」），再让同一个模型写各幕的话；这一步失败不影响信。每运行一次多一行，就是历史。补做、立即运行都走年度总结那个定时任务的流程和模型（`schedule.rs` 的 `atlas_run`）。
- **模型写的只有话**（几幕、各写什么见 `requirements.md`「Atlas（年度总结）」）：数字、日期、图都不归它，它写的 Rust 都照快照查一遍，不认识的丢掉；读不出来告诉它原因再要。自建模块那一幕的排法见 `youzi/made.rs` 开头。各幕的数据都在 `data.rs`，和年度总结读的同一份。
- **画在设计里**：设计是同步来的冻结快照（`src/vendor/pomelo/atlas/<名字>-<版本>/`）；每一行记着做的时候的设计，打开时由 `modules/atlas/designs.ts` 找来画（找哪一份见 `design.md`「Atlas」）。
- **小报放在 frame 里**（`Journey.tsx`）：frame 的文档是快照自己带的（样式、上墨的脚本都是冻结那天的），里外的样式互不影响，旧的小报不会被后来的界面改动弄坏。同一个来源，页面直接把数据交给它，它把点了什么告诉页面。
- **信存两份**：对话里、`year_atlas.letter`；明年写信带的「去年的年度总结」和对话删了时都读后者（`yearly.rs` 的 `letter_of`）。
- **删除只在页上**（`atlas_delete`，进撤销）。首页的信封由 `atlas.rs` 的 `envelope` 决定。

## 系统通知

- **规则都在 `notify.rs`**（细则在它开头）：待办、投递、来日方长由柚子的后台循环每 30 秒看一次，到了时间、今天还没看过就算一遍、发一条；算的和页面用同一套函数。定时任务在 `schedule.rs` 跑完时发。
- **点通知**：通知带着要打开的东西，点了叫出主窗口，发和速记小窗一样的事件（`capture-open`、`capture-chat`）。
- **只有真的才弹**（UserNotifications 只能在 .app 里用），其余只写到终端；临时数据目录的打包版带 `POMELO_REAL_NOTIFY=1` 能试真的（和正式版同一个 App，系统的允许共用）。

## 首页、月历和每天的例行检查

- **首页、月历**：`home.rs`、`calendar.rs` 每次打开时现算（只读，不存），隐藏的模块不算。
- **例行检查**（`daily_sweep`）：每天第一次打开、或开着过了零点：每日备份；把时间段结束或已打满的打卡任务标为完成；记上到日子的固定支出（漏掉的一起补）。窗口关着、只留菜单栏时，每日备份每 5 分钟查一次，另两件等下次打开窗口补。不进撤销（撤销了下次还会再做）。

## 图表

都在 `src/shared/charts.tsx`，自己用 SVG 画，不引入图表库；样子见 `design.md`。

## 占用空间和内存

App 本体约 25 MB，运行时约 130 MB 内存；`pomelo.db` 很小（十年也在几百 MB 以内），真正占空间的是附件。

## 柚子（AI）

- **旧名**：代码里剩下的 picky 只为读旧数据（数据文件 38 之前的 `picky`、旧文件夹名 `youzi/store.rs` 的 `adopt_old_names`、旧偏好键 migration 0038），不改名；聊天记录里的「小拾」不动。
- **柚子服务**（`provider.rs`，细则在它开头）：统一的「发消息、流式收回复」，背后是本地模型（8080，OpenAI 兼容）或六家云端（固定的官方 HTTPS 地址）。回复、思考、用量边收边交给界面，定时存进 `youzi.db`，中断时存下已收到的；出错只显示固定的中文提示。每个对话同时一个回复（`chat.rs` 的 `Runs`）；本地模型一次只答一个、在这里排队，云端不排队。云端还没收到内容就出错时等几秒重发，最多 3 次。
- **思考**（调用时的规矩见 `CLAUDE.md`「柚子（AI）」）：各家的开关、强度怎么发在 `provider.rs` 开头；模型不收某个设置就去掉再发一次、记住到退出；阿里云内容审核拦下时直说、不重试。存「开关 + 强度」或 `cloud::Level` 的偏好见 `schema.md`；中间步骤（对话压缩、读长文件、年度总结的每月阅读）用模型的默认。
- **本地模型**（`runtime.rs`，细则在它开头）：照 `~/Documents/LLM` 的清单和 `llmctl.sh` 启停；聊天模型同时一个（8080），向量（8081）、重排（8082）用时自动启动；柚子启动的闲置 30 分钟后停、退出时停，别处启动的不碰。
- **读文件**（`files.rs`、`office.rs`、`reading.rs`、`pdf.rs`；各格式见 `office.rs` 开头）：添加时读一次存成分段文字；大表格只存表头和前几十行（「查表」从副本现读）；网页在 Rust 里下载。图片（包括文档里的图）只给云端，发前用 `sips` 缩小，在数据库锁外读。
- **挑选和检索**：记忆、资料段落、知识库都是先按向量相似度取一批，再用重排挑至多 6 条，重排用不了只按相似度（数量和分数线见 `memory.rs`、`index.rs`、`knowledge.rs` 开头）。**出处**（`cite.rs`）：一次回复拿到的统一编号，随回复存进 `youzi.db`；「读整条」读前再按规则查一遍。
- **查数据**（`data.rs`）：模型的工具调用；Rust 先查能不能读、能不能发云端，再查、截短交回。钱来钱往的云端按两块算（收支、资产，`cloud_key`），查询、页面、卡片、搜索结果各按自己那块挡。
- **改数据**（`act/`，细则在 `act/mod.rs` 开头）：用模块自己的函数在撤回的事务里试写，前后一比就是卡片，告诉模型的话照抄卡片；确认时整张卡片一步 ⌘Z。
- **问我**（`ask.rs`）：后台任务停在 `ask::Gate` 上等，回答变成工具结果接着走，期间生成位置一直占着；退出后的回答当新消息发，所以不存模型的中间状态。
- **面板帮手**（`assist.rs`）：一问一答；Rust 写好提示（界面只传编号），回答读成字段再查一遍才交给面板（日期真实、名字对上、链接是句子里写了的）。呦呦漫游在 `data.rs` 里「只帮忙、不读」（`HELP_ONLY`）。
- **插件库**（`skills/`）：开着、Key 齐了、并且「自己决定用」或这一条点名要它才给；点名的在系统消息里写明必须先用（不用 `tool_choice: required`，有的服务不认）。查到的都编号成出处。
- **定时**（`schedule.rs`）：后台循环每 30 秒看一次到点的，和发消息走同一条路，只是只读、系统消息说我不在；要用的本地模型没开就先启动，跑完只停自己启动的。
- **今日简报**（`brief.rs`）、**年度总结**（`yearly.rs`）：Rust 先用 `data.rs` 读好要用的再发请求。简报一次请求，不占对话的生成位置；年度总结放不下就先按月整理要点（没变的月份直接用）再写全年。「定时」里的流程图是 `yearly.rs` 的 `FLOW`，改流程要一起改。
- **知识库**（`knowledge.rs`）：`youzi.db` 只记库、来源、开关这类选择（能撤销）；文件清单、分段和向量在 `search.db`（后台扫描只写它，不挡 ⌘Z）。不监听文件，定时比大小和修改时间；文字没变的段落沿用旧向量。「打开文件」先查路径没出文件夹。
- **助手**（`assistants/`）：设置和上一次的结果在偏好 `assistant.<名字>`，不进撤销和导出。Translator（`translate.rs`）长文按段切块依次流式翻，后一块带上前一块译文的结尾；在后台任务里跑，页面轮询。呦呦漫游的按中文起草（`site.rs`）用同一套，结果只在内存。
- **Pub2MD**（`assistants/pub2md/`，流程在 `job.rs` 开头）：直连 Mathpix，每步存进 `.pub2md/job.json`（「继续」从那里接着做）；简繁用 ferrous-opencc；预览用 mathpix-markdown-it，放进不能跑脚本的 iframe；模拟版用 PDFKit 读出的文字代替 Mathpix。
- **Work**（`work.rs`、`word.rs`、`skills/toolbox.rs`）：.docx 自己生成。改文件只改 Rust 列出的文件，提议逐段查过放进卡片（记下指纹），写入时文件变了就不写；Word 只改变了的字，其余原样，改不了的拒绝。**Excel 工具箱**（`excel.rs`）：不加新库（zip + quick-xml；calamine 只读），只重写动到的表，挪不好的拒绝，写入时要求打开时全部重算；读大文件在锁外。
- **DevJanitor**（`assistants/janitor/`，规则见 `CLAUDE.md`「DevJanitor」，细则在各文件开头）：
  - **假 Mac**（`fake.rs`）：大文件是稀疏文件，`.janitor-fake.json` 写着假的磁盘大小、在跑的程序、Docker；删除和废纸篓都在里面，命令不真跑。
  - **扫描**（`scan.rs`）：8 个线程走一遍，只看名字、大小、时间；真磁盘约 135 万个文件、17 秒。结果一次整个换进 `janitor.db`。问系统（`system.rs`）只用系统自带的命令和系统库，不加依赖；完全磁盘访问权限看 TCC.db 能不能打开（不弹框）。
  - **克隆和可清除的空间**：APFS 的克隆（Peck 的评测工作区把预热好的 `target/`、`node_modules` 用 `cp -c` 克隆进每个工作区）共用块，按块算会把同一份算好几遍（11 月一次算出 618 GB 的整块磁盘）。真磁盘上每个文件用 `getattrlist` 问独占的字节、克隆家族的编号和成员数：独占的照算，一整家完全一样的克隆共用的部分像硬链接一样只在第一次碰到时算；改过的克隆只算它独占的（共用的算在原件那里）。一项能腾出多少 = 它独占的 + 整家都在它里面的。剩余空间用 CoreFoundation 的「重要用途可用」（含可清除的），和系统设置一样。
  - **Agent**（`agent.rs`）：借柚子的对话和生成的路，换成 DevJanitor 的模型、SKILL 和工具（不给柚儿园的数据）；只读命令过了 `readonly.rs` 的清单才跑。卡片在锁外查好再存进 `janitor_cards`。
  - **受保护**（`rules.rs` 的 `Protected`）：页面只传条目编号，路径从 `janitor.db` 取。柚子查它用只读工具 `pomelo_janitor_info`，和 Agent 看的同一份文字。
  - **VAULT**（`rules.rs` 的 `sealed`）：名字去掉扩展名是 vault（不分大小写）的，扫描不进、不算，Agent 查文件夹和找文件时不列，只读命令的参数碰到就不跑、输出里提到的行删掉，卡片不提议，备份不碰。
  - **备份**（`backup/`，细则在 `backup/mod.rs` 开头）：每 3 秒看一眼 `/Volumes`，按卷的 UUID 认我的移动硬盘（`diskutil` 每块盘只问一次），插上就把每个任务（清单里勾上的「下载」文件夹对硬盘上同名的文件夹，`settle` 照清单和两边现有的文件夹算）两边对一遍放在内存里；页面每几秒来取。对比只读名字和元信息，名字按 NFC 对（Mac OS 扩展存成另一种写法），时间按秒比。复制按 1 MB 一块写到旁边的 `.pomelo-part-…`（能在两块之间停下），再用 `fcopyfile` 带上标签等扩展属性、补上时间，最后改名放到位，硬盘上不会有半个文件；被覆盖的先在硬盘里挪进 ARCHIVE（同一块盘，只是改名）。一次只跑一个，复制前先算够不够地方。实测：真硬盘约 11,000 个文件，冷读一遍约 4 秒，之后不到 0.3 秒。
  - **PikPak**（`backup/rclone.rs`、`pikpak.rs`）：用 Homebrew 的 rclone（找 `/opt/homebrew/bin`、`/usr/local/bin`）。配置单放在数据目录的 `rclone/`，由 rclone 加密（`config encryption set`），密码随机生成、只存在钥匙串（「Pomelo OS Agent」那一项），每次只通过环境变量交给 rclone，不上命令行、不落盘；PikPak 的账号、密码和 rclone 会刷新的 token 都只在这份加密配置里，所以「授权只在钥匙串」。对比：硬盘上勾了的文件夹读名字和大小（和 rclone 一样只跳过 VAULT 和系统小文件），PikPak 那边用 `rclone lsjson` 只列这几个文件夹，在 Rust 里照 rclone 同步的规则算（`pikpak::cloud_diff`）：同路径同大小的一样、大小不同的重传、同名同大小换了地方的挪、只在 PikPak 上的挪进 ARCHIVE；放在内存里，页面来取。同步是 `rclone sync 硬盘 → pikpak:文件夹`，只带我勾的文件夹（`+ /名字/**`，其余 `- **`，所以 ARCHIVE、没勾的、根目录的文件两边都不碰）；过滤掉 VAULT（不分大小写、任何一层）和 `diff.rs` 的那些系统小文件；只比大小（`--size-only`：PikPak 的修改时间是上传的时间）；改名按文件名和大小认（`--track-renames-strategy leaf`），在 PikPak 上直接挪；多出来的和被换掉的挪进 `文件夹/ARCHIVE/日期`（`--backup-dir`，在同一个文件夹里，靠过滤规则排除），而且 rclone 只要有一个文件没成就不挪；挪进 ARCHIVE 的超过「比对出来的 + 100」就停（`--max-delete`，停之前已经挪的就在 ARCHIVE 里）；名字传成 NFC（rclone 对名字时 NFC、NFD 算一个）；PikPak 上的名字原样读写（环境变量 `RCLONE_PIKPAK_ENCODING` 去掉 rclone 默认里 Windows 不让用的那几个字符：默认会把全角的「：」「？」当成半角的替身，PikPak App 传的对不上）；PikPak 不收的半角 : ? * " < > | \\ 用 rclone 的 `--name-transform` 换成 _（PikPak App 也这么做；对比时 Rust 照同样的规则换，`rclone::cloud_name`），换完撞名的文件夹不同步；传了却回来「名字(1)」的（`uploaded file name mismatch`，名字被中断的上传留下的半成品占着，列表里看不见），把那一层文件夹用目录挪动整个挪进 ARCHIVE（不带过滤规则，rclone 才做目录挪动，看不见的跟着走），再带过滤规则把看得见的一个个挪回来，然后再同步一遍；进度读 rclone 每秒一行的 JSON，挪了几个数 `Renamed from` 那几行（它的统计把挪进 ARCHIVE 也算成改名）。一次只跑一个，App 退出时停掉 rclone，下次再跑只传还没传的。开发版、测试和临时数据目录：「PikPak」是数据目录里的一个本地文件夹，配置用固定密码（不碰钥匙串），上传走 rclone 普通的复制，限速和进度和真的一样。
  - **柚子的线索**（`backup/hints.rs`）：每次对比顺带把整块硬盘（ARCHIVE 除外；清单里没勾的读了只算大小，不进线索）和「下载」里的文件夹各读一遍名字和元信息，同名、同大小、修改时间差 1 秒内的算一样：电脑上的文件夹按相对位置给硬盘上的文件夹「投票」，过半的就是它在硬盘上的地方；任务里改名的按文件夹归在一起。只在硬盘上的算归档，不进线索；重复不找。线索和对比一起放在内存里，写进 Agent 的「现在的情况」，提议的做法写成「可以：…」一行（模拟模型照这些行出卡片）。卡片（`cards.rs`）提议时查一遍、点了再查一遍，只认提议时那块硬盘。自动看一眼（`look.rs`）按线索的指纹去重，问题写进 Agent 的对话，由它的模型照常回答；上次看的指纹和回答编号存在偏好里，「备份」页从那条回答读字和卡片。
- **体检**（`health.rs`）：只读、现算，结果用全局搜索的 `Hit`，页面照 ⌘K 打开。**空间管理**（`storage.rs`）：删副本只删文件、留记录（「文件已删除」）。
- **云端 Key**（`cloud.rs`）：界面只显示「已保存」；App 打开时读一次钥匙串那一项、放在内存，查「有没有存」从不弹框。
- **开机自动启动**（`autostart.rs`）：Tauri 的 autostart 插件写一个带 `--hidden` 的 LaunchAgent（不用 AppleScript），这样打开时窗口第一次画出来前就藏起来；每次打开让登录项跟着设置里的开关。
- **Seeker**（`assistants/seeker.rs`、`seeker/`，细则在 `seeker.rs` 开头）：打字停一下自动存（`seekerDraft.ts` 排队），一次保存一步撤销。模型每次只调一次、没有工具；发给模型的草稿带短编号，回答按编号查过才给差异或变成批注，对不上的略过。规则发给谁只写一处（`seeker/model.rs` 开头），调用和规则页都用它。页面的调用在 `seekerJobs.tsx`（不跟页面卸载）。岗位关联、「做简历」、公司资料都先读 `pomelo.db` 再锁 `youzi.db`，不嵌套。Word 照内置模板（`src-tauri/templates/`，只留格式）里样板段落的格式一段段写（`seeker/output.rs` 开头）。
- **Rater**（`assistants/rater.rs`、`rater/`，细则在 `rater.rs` 开头）：题目、练习、面试都是一条一行 JSON，保存按版本号查、旧的进历史、一步撤销。题库 SKILL 只调一次模型、没有工具，模型回 JSON，Rust 按编号查过再交给页面或确认卡片。复盘 · 记录借一个私密、不进列表的柚子对话填表（`record.rs`）：聊天和填表不进撤销，存成复盘一步撤销。

## 见字如面的导出（PDF、Word）

- **一个隐藏的窗口**（`print.rs`，页面 `print.html`；细则在 `print.rs` 和 `src/print/layout.ts` 开头）：导出时 Rust 开它，交给它这次的篇目、存到哪、样式，做完关掉；一次只导一批。
- **排版用编辑器自己的画法**（`bear.ts` 的 `drawDoc`），所以 Bear 的写法在 PDF 里和 App 里一样；按行切成 A4 页。
- **PDF**：WebKit 一页页画成 PDF（字还是字），engine-pdf 合成一份，加上目录的跳转、网址链接、书签和标题。**Word**：页面把排好的行拆成段落和文字片段交给 Rust（`articles/docx.rs`）写出 .docx。
- Markdown 不经过这个窗口：Rust 直接拷正文和附件。
