# 技术架构

> 怎么做。给 Agent 的规则摘要在 `CLAUDE.md`，功能见 `requirements.md`，表和字段见 `schema.md`；细则在各文件开头。

## 基本原则

- **个人、轻量**（见 `CLAUDE.md`「最高原则」）：能用简单办法的不用复杂办法。
- **数据能长期保存、随时带走**：App 随时能换掉，数据不丢。
- **默认离线**：哪些地方联网见 `requirements.md`「总体」。

## 技术选型

| 部分 | 选择 |
|---|---|
| 桌面应用 | Tauri 2 |
| 界面 | React + TypeScript + Vite，Tailwind CSS；少量自己写的组件，不用组件库 |
| 数据层 | Rust + SQLite |
| 柚子（AI） | Rust 用 reqwest 连模型、收流式回复；消息排版用 markdown-it、highlight.js、KaTeX |
| 见字如面 | 自己的 `LiveText` 加 Bear 的 Flavor；highlight.js、KaTeX（和柚子共用）、Mermaid（真有流程图时才加载）；从 Bear 搬家调只读的 `bearcli`；PDF 用系统的 WebKit 和 PDFKit，Word 自己写 XML（zip），不加依赖 |
| 读写文件 | macOS 自带的 PDFKit（PDF）、`textutil`（读 Word）、`sips`（缩图片）；`zip` + `quick-xml`（PowerPoint、改写 Word），`calamine`（表格），`dom_smoothie`（网页正文） |
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

- 界面只调 Rust 命令（如 `books_list`）；柚子的工具也是 Rust 命令（只读查询、「提议」），碰不到数据库。哪些永远不做成工具见 `CLAUDE.md`「柚子（AI）」。
- 用模型都经过「柚子服务」（聊天、定时任务、简报、润色补全、翻译）；本机别的程序用本地模型直接连 8080。
- **命令在哪条线上跑**：`pub fn` 在主线程上跑，跑着时键盘、触控板的事件得等；页面反复调用或读得多的写成 `pub async fn`（后台线程，数据库本来有锁）：见字如面保存和跟着刷新的（`articles_list`、`articles_overview`、`articles_titles`、`article_get`、`article_create`、`article_update`）、柚子链接的 `links_lookup`、`links_suggest`、`links_backlinks`、`links_thing`（各读一遍全部笔记和文章）、`notes_list`、`notes_overview`。后台命令先后不定，所以见字如面的保存在页面里排队发（`Editor.tsx`）。

## 数据存放在哪里

| 位置 | 放什么 | 备份 |
|---|---|---|
| `/Applications/Pomelo OS.app` | App 本体 | 不用 |
| `~/Library/Application Support/Pomelo OS/` | 真实数据：`pomelo.db`（生活数据和设置）、`youzi.db`（柚子）、`backups/` | **要** |
| 同上的 `attachments/` | 附件（原文件的副本） | 不进每日备份，交给 Time Machine 这类工具 |
| 同上的 `youzi-files/` | 柚子的附件和项目资料（另存一份，不和上一行共用） | 不用 |
| 同上的 `search.db`、`janitor.db` | 搜索索引、DevJanitor 上次扫描的结果 | 不备份、不导出，删了重建 |
| 同上的 `icons/` | 一网打尽的网站图标（一个网站一个文件） | 不备份、不导出，删了重新取 |
| `~/Library/Application Support/Pomelo OS Dev/` | 开发用的测试数据；`janitor-sandbox/` 是 DevJanitor 的「假 Mac」 | 不用 |
| macOS 钥匙串 | 「Pomelo OS Agent」（云端、插件库、Pub2MD 的 Key 合成一项）、「Pomelo OS 芝麻开门」（Touch ID 用的钥匙） | 不进备份 |
| `~/Documents/LLM` | 本地模型、模型清单、启停脚本 | 由 Claude 维护；Pomelo 只读清单、调脚本 |
| `~/Documents/GitHub/personal-website` | 个人网站（呦呦漫游编辑、发布它） | 独立项目，自己的 git 和 GitHub 仓库 |

数据库别放进 iCloud、Dropbox 这类同步文件夹（可能弄坏）；异地保存就复制**备份文件**。

## 开发版和正式版

- 同一份代码，只差数据目录：正式版是「应用程序」里的 App（真实数据，那里只留一个 Pomelo OS），开发版从代码运行（测试数据，随便清空）。`pnpm app:install` 更新正式版（什么时候装见 `CLAUDE.md`「正式版」），新版第一次打开先自动备份再升级。
- **签名**：固定的自签名证书「Pomelo Self-Signed」，系统认得是同一个 App，更新后钥匙串里的 Key 还能读。自签名没有团队编号，钥匙串只按版本认，所以每次更新要再授权：打开新版时弹一次（Key 合在一项里），芝麻开门第一次用 Touch ID 前弹一次；输登录密码、点「始终允许」。完全不弹得换苹果的开发证书（要 Xcode 或付费账号），定了不换。证书丢了就重建：钥匙串访问 → 证书助理 → 创建证书，名称「Pomelo Self-Signed」、身份类型「自签名根证书」、证书类型「代码签名」（换了证书，钥匙串里存过的 Key 要重新授权）。
- **真的和模拟的**：只有正式数据目录的打包版连真模型、扫真磁盘、用真的网站文件夹、取网站图标、弹系统通知、登记开机启动；开发版、测试、设了 `POMELO_DATA_DIR` 的打包版都是模拟的（怎么模拟见各节）。单独打开真的：`POMELO_REAL_MODELS=1`（如 `POMELO_REAL_MODELS=1 pnpm app:dev`，先问我）、`POMELO_REAL_SITE=1`、`POMELO_REAL_NOTIFY=1`。
- **编译缓存**：`src-tauri/target` 每换一种编译方式多一份、只增不减。开发编译时依赖不带调试信息、自己的代码只带行号（完整编译测试一次约 3 GB）；`pnpm check`、`app:dev`、`app:build`、`app:install` 和开会话时超过 20 GB 整个清掉（`scripts/clean-build.sh`，从头编约一分半）。

## 数据库升级

- 改结构只新增编号脚本（`src-tauri/migrations/`；`youzi.db` 有自己的一串和版本号），用过的永远不改。
- 启动时要升级就**先自动备份**再逐个执行；数据库比 App 新（装回了旧版）时拒绝打开。
- `search.db`、`janitor.db` 只有版本号、不升级：对不上就删了重建。

## 备份与导入导出

| | 快照备份 | 数据文件（JSON） |
|---|---|---|
| 是什么 | 整个数据库的副本，在 `backups/` | 所有模块的数据，人能看懂（格式见 `schema.md`） |
| 用途 | 出问题时恢复 | 长期归档、换工具、从 Excel 导入 |
| 什么时候做 | 每天第一次打开、升级前、覆盖导入前、恢复前自动做；也能「立即备份」 | 「设置 → 导出…」 |
| 怎么用回来 | 设置里的备份列表，「恢复」 | 设置里「导入」 |

- **留多少**：每日备份 `pomelo.db` 留 30 份、`youzi.db` 7 份，更早的每月留最后一份；两个库分开显示、分开恢复。升级前、导入前、恢复前的各留 3 份（做每日备份时清）；手动的不自动删。
- **恢复**把当前数据整个换成那份（先备份当前的）；老版本的恢复后自动升级，损坏的或比 App 新的拒绝。**导入**有一条不合格就整个不导入；覆盖前先确认、备份。
- 文件名写着时间和原因：`pomelo-20260929-093000-daily.db`，原因还有 `manual`、`before-v7`（升到版本 7 前）、`before-import`、`before-restore`；柚子的以 `youzi-` 开头。

## 附件

- **存**：Rust 把文件复制到 `attachments/`（随机名、只有我的账户能读写），按 SHA-256 同样的内容只存一份；界面只拿到编号、名字、大小。
- **看**：图片走 `pomelo-file://localhost/<编号>`（Rust 只按编号给已存下的图片）；其他文件交给 macOS 用默认程序打开。
- **拖进来、粘贴**：交给落点所在的那块（在写的笔记、开着的任务面板、目标对话框；都不是就给最近打开的那块）。粘贴的截图没有路径，界面把内容直接交给 Rust。
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

树（一网打尽，念念不忘和见字如面的标签；`BookmarksPage.tsx`、`TagTree.tsx`，不用 `shared/sortable`）：一次拖动交给 Rust「放进哪个文件夹 / 哪一层、放在哪一条前面」（`bookmarks_place`；标签换了一层先改名再排，顺序按完整路径记在标签设置里），Rust 查过文件夹不进自己里面、网址不放最外层，再重新编号，一步 ⌘Z。落点怎么算写在这两个文件里。

## 写字的输入框

`src/shared/editor/` 的 `LiveText` 是共用的底子：只收纯文字的可编辑框，文字是唯一的数据，每改一次按行重画、按位置放回光标（`editable.ts`）。只重画变了的行：Flavor 每行给一段 HTML，和上次比，只换了样式名的只改样式名，浏览器自己打出来已经一样的不碰，行数变了只重画中间那段；光标位置真变了才放回。输入法拼字时整个框归它：不重画、不读回，也绝不拿没读回的字重画（别处要重画、换行、⌘Z、存，都先读回）；拼完等下一帧再读（可能上屏一半接着拼），窗口藏起来没有帧时 0.12 秒后读。自带 ⌘Z、粘贴、列表续行、⌘ 快捷键加标记；光标下的小列表固定定位，挂在页面或所在对话框上，不被滚动的框裁掉。刚回车出来的空行先按「要写的那一段」画（整行高、带段距），光标离开才缩回。Flavor：`lite.ts` 念念不忘的格式（`LiteEditor`）；`markdown.ts`（能给「只有一张图的行」配图片地址，呦呦漫游的 `MdEditor.tsx` 用；写提示词的 `MdField.tsx` 没有图片）。

- **右键的那条纸**（`MdMenu.tsx`）：`LiveText` 自己接右键，列出 Flavor 的 `formats`（包住选区、给每行加开头、加一行，或 `act` 自己做），再是两个图标库；新 Flavor 写上 `formats` 和 `linePrefix` 就有。同名 `group` 收成一行，`lead` 的是粗体。「柚子链接…」是共用的 `shared/links/LinkPicker.tsx`，`LiteEditor` 传了 `links` 就有。
- **字里的图标**（`inlineIcon.ts`）：`:pm-<名字>:`（名字是图标库的键）画成光标进不去的小块（`contenteditable=false`，里面藏着这几个字，框里、复制、存下的文字都不变）；`editable.ts` 的 `pointAt` 把落在小块里的位置放到它前后。`NoteBody`、`InlineMd` 也画它，笔记的一行预览（Rust `notes::excerpt`）去掉它。`:名字:`（没有 `pm-`）是网站的内容图标，Pomelo 不画。各家浏览器对这种小块不一样，改了在 Safari 里测（`testing.md`「写字的框」）。
- **见字如面**（`modules/articles/bear.ts`、`Editor.tsx`）：Flavor 知道光标在哪行（`caretAware`），换行就重画（大多只是两行换样式名）：那一行（和所在的代码块、公式、表格、提示框）露出记号，别的变淡；待办的框、图片、公式、流程图、提示框的图标画成「预览」（`data-pv`：不算进文字，点一下光标进它的源文字）。标题的 `#` 和空格是光标进不去的小块，挪到左边空白里画级别。代码上色、公式、流程图按源文字缓存，一次重画每块只算一次；流程图异步，画好再重画一次。折叠是页面的状态（按那一行的文字和第几次出现记），不进正文；从 Bear 搬来的折叠记号只决定打开时折着哪些。专心写时正文的节点整个挪到 `<body>` 底下（React 的 portal 指着同一个节点），编辑框不重建，光标和 ⌘Z 都在。
- **见字如面的双指滑动**（`modules/articles/swipe.ts` 的 `Glide`）：滚轮事件不带「手指还在不在」，只能从数字里认；App 忙时事件晚到、几个并成一个，看着像「停了又开始」，会收两栏。所以只有手真会做的才算新的一次：往回滑；真的停过（按事件自己的时间，且页面醒着：滑动期间每 8 毫秒记一拍，睡过去的停顿不算）；滑行中手指放回去（先一小段真的停顿、接着位移很小、然后速度连着两次变快）。收过一栏后 0.3 秒内都不算新的。
- **图标库**（`shared/iconLibrary.tsx`）：存成 `i:<键>`，键是图标的文件名；念念不忘早先的 48 个键（`pin` 是地点、`notebook` 是带笔的本子…）原样认，被它们占了名字的两个文件另起键（`pushpin`、`notebook-plain`）。Rust 只管长度（`common::clean_icon`），不认识的键只是不画。

## 界面外框

- **开着的页和前进后退**（`src/app/tabs.tsx`，只在界面里）：用路由地址记一个模块停在哪（`#/finance/assets`，子页是第二段，`subpage.ts`）；‹ › 记走过的页（最多 50 步）。贴纸簿、柚子瓣、全局搜索、顶栏走 `open`（开着的拿到最前，没开的新开），页面里自己的跳转由外框记下。鼠标侧键用 `mouseup` 的第 4、5 键。开着的页存在设置 `ui.tabs`；库里有 `dev.open` 时这一次启动直接停在那一页（测试用，见 `testing.md`「真实窗口」）。
- **贴纸簿和柚子瓣**（`dock.ts`、`StickerBook.tsx`、`Bloom.tsx`；代码里叫 dock）：设置 `ui.dock`（按模块键记）、`ui.module_hidden`、`ui.module_look`（`LookPicker.tsx`、`moduleLooks.ts`），都不进撤销（形状见 `schema.md`）。哪个分类都没有的模块进 `dock.ts` 的 `DEFAULT` 定的分类。助手在导航里各是一个键（`translate`、`pub2md`、`seeker`、`rater`、`importer`、`janitor`、`atlas`）。柚子瓣是同一份设置的另一种画法（SVG），编辑时只更新内容、不重播动画。拖动用鼠标事件（`drag.tsx`：落点 `data-drop`、插在哪个前面 `data-mod`）。实时小字是只读命令 `dock_live`（`dock.rs`）。
- **柚子**（`Pomelo.tsx`）：SVG 组件，表情是参数，动画用 CSS（`frame.css`）。心情：今日简报让模型在最后一行写「心情：…」，Rust 取出存进 `briefs.mood`（模拟模型给「很忙」）；`youzi_brief` 带上写好的时间（`moodAt`），界面只在之后一小时内用它。在写读 `youzi_status` 的 `generating`；出错由界面报（`reportProblem`）：每日备份失败；发布和 Pub2MD 开始后登记检查（`watchJob`），外框每 2 秒问一次直到做完。
- **速记小窗**（`capture.rs`、`src/capture/`，细则在 `capture.rs` 开头）：同一个 App 的第二个窗口，第一次用才建、之后只藏起来（重建要半秒多）；⌥Space 用 Tauri 的全局快捷键插件（开发版也抢这个键）。问柚子的对话用 `library.rs` 的 `unlisted!()` 挡在列表、搜索、⌘K 外面；翻译和 Translator 页共用 `translator.tsx`。和主窗口用事件来回通知（`capture-saved`、`capture-chat`、`capture-open`）。
- **Mac 菜单栏**（`menu.rs`）：自己建的菜单，代替 Tauri 默认的（它的 ⌘W 是关窗口）。「页签」菜单发 `app-menu` 给主窗口，由 `App.tsx` 的 `useTabKeys` 做；焦点在速记小窗时只认 ⌘W / ⇧⌘W（收起）。⌘1～⌘9 的名字在置顶变了时由页面发来（`app_menu_index`），Rust 重建菜单。⌘⇧[ ⌘⇧] 只在页面里认。

## 打开链接

带协议的（`https:`、`mailto:`、`bear:` 等）直接交给系统；`/…`、`~/…` 当本机文件；其他的（如 `www.xx.com`）补上 `https://`。

## 模块之间

改好几张表的操作放在一个事务里（`common::Tx`，可嵌套：模块互相调用、导入时调各模块都不冲突），中途出错全不算。

- **一网打尽**（`modules/bookmarks.rs`）：文件夹和网址一张表，界面拿整张表自己搭树。网站图标（`icons.rs`）：页面问「有哪些图标」时把 `icons/` 里缓存的转成 data URL 一起给，同时在后台去取还没有的（一次 4 个；先读首页的 `<link rel="icon">`，再试 favicon 的常见位置，子域名都没有就试主站；只认真的图片，最大 300 KB），取到发 `bookmark-icons` 让页面刷新；取不到的记进 `failed.json`，7 天后再试。
- **五花八门、循序渐进**（`custom.rs`、`lists.rs`、`progress.rs`）：建出来的每个当一个模块，键 `list/编号` / `progress/编号` 也是页面地址；循序渐进的类型是 `kinds` 里 `module` = 这个键的一组（和随意画画同一套）。选项和类型按在列表里的位置配颜色（`--tag-1`～`--tag-8`，超过 8 个从头再来），不存。界面的模块列表 = 固定模块 + 从 Rust 读到的建出来的。五花八门每条的字段值存成一份 JSON，规则在 `lists.rs` 里查；排序、筛选只在页面上。柚子用 `pomelo_lists`、`pomelo_progress` 查，工具说明里不写模块名（不带个人数据）。
- **导入账单**：账单和行在 `modules/finance/import.rs`（`pomelo.db`，进撤销和导出）：每行留读到的原文和我核对后的草稿，确认后才写进 `fin_entries`（一步撤销）；候选没决定的不让入账。读账单的 **Importer**（`youzi/assistants/importer.rs`，细则在它开头）只用云端模型（`schedule::route`），只有三步：`youzi_importer_extract`（Excel 每 60 行一块带表头，PDF 每 4 页画成图一块；全局同时最多 4 个请求）、`youzi_importer_match`（查重，合并要程序对上金额、币种、日期、付款渠道 `CHANNELS` 和分类，对不上的列进候选）、`youzi_importer_tidy`（再整理）；写之前逐行比对，运行期间我动过的行不写。页面的任务在 `importJob.ts`，离开页面也跑完。
- **柚子链接**（`modules/links.rs`，规则只写在那里；界面 `src/shared/links/` 照着解析来画）：不另存表，链接就是正文里的 `[[…]]`。`Names` 一次读出所有能被链到的东西和名字的对照，`Index` 一次扫完正文算出谁链到谁（含笔记的引用 `note_refs`）。页面用 react-query 的 `["links"]` 缓存，笔记、文章、书影游一改就作废（`linksChanged`，连念念不忘的列表一起：每条笔记带着它的柚子链接，缓存不会自己过期）。改名跟着改是 `follow_rename`（同一个事务），导出时嵌进来的笔记变引用是 `quote_embeds`。
- **玩物有志**（`modules/games.rs`）：照摸鱼读读写，界面和书影共用 `shared/collection/`（`CollectionConfig` 的 `platforms`、`runs`、`noCreator`、`statuses` 开关多出来的几样）。平台是 `kinds` 的第二组 `game_platform`（列叫 `platform_id`，`kinds.rs` 的 `TABLES` 写成 `games.platform_id`）。「搬到玩物有志」（`from_list`）在一个事务里把五花八门清单写进 `games`、删掉清单（对法见 `schema.md`）。
- **笔记转待办**：任务记下来自哪条笔记（笔记删了变空）；数据文件里用笔记的记录时间对上，导入时先导笔记再导待办。柚子「转为待办」：模型定几件、每张怎么写，Rust 一张张查过再列出；勾选的整组交给 `todo_tasks_from_note`，照任务面板的规则逐张建、一步 ⌘Z（有一张不合规矩整步不做）。
- 首页、搜索、柚子读各模块都走同一套：`home.rs`、`search/sources.rs`、`data.rs`（如一网打尽的 `pomelo_bookmarks`）。

## 搜索和索引

- **全局搜索**（⌘K，`search/mod.rs`）：Rust 在 SQLite 里按子串逐条比对每个模块要搜的字段（所以中英文都能找到词中间的字）。界面跳到那个模块，由模块页面打开那条（`src/shared/open.ts`）。
- **一个入口**：`search/query.rs`（细则在它开头），「意思相近」和柚子的搜索都调它：给一句话、范围、方式（按字、按意思、合并）；隐藏的模块、云端规则在这里挡，向量或重排服务用不了时照样按字返回。按字用 FTS5（中文两个字一组）；合并后让重排模型给前 30 条打分，0.1 分以上的排前面，其余保持合并后的顺序（短词它全给接近 0 分，全按它排会把对的挤下去）。
- **索引和原始数据分开**：`search.db` 只放算出来的（表见 `schema.md`）；「哪些要建索引」这类选择留在 `youzi.db`。
- **后台更新**（`search/indexer.rs`）：自己的只读连接，每 2 秒看一眼两个库改过没有（SQLite 的 `data_version`，撤销、导入、恢复都算），有就按指纹比对，只写变了的、删掉没了的；不在命令里挂钩子。
- **向量什么时候算**：只在向量服务本来开着、或按意思搜 / 柚子检索要用时补算，不为改一条记录专门启动。重建（`search.db` 新建、换向量模型、换文字规则）例外：自动启动服务，后台从新到旧算完，期间照常按字找。
- **速度**：向量压成 8 位整数载入内存逐条比（20 万段约 200 MB）；20 万条实测按字约 0.06 秒、按意思约 0.02 秒。再大就换近似检索，所以向量检索做成能换的部件。
- **全量读取**：年度总结这类要看全部数据的不经过检索，直接用各模块的只读函数读全。

## 呦呦漫游（个人网站）

- **内容只在网站文件夹里**（见 `CLAUDE.md`「个人网站」）：`site.folder` 记位置（默认 `~/Documents/GitHub/personal-website`），不存副本；网站的 git 历史就是它的历史。格式照它的 `content.config.ts` 和现有文件。
- **运行方式**（`site/mod.rs`）：真的（见「开发版和正式版」，或 `POMELO_REAL_SITE=1`）用真文件夹、真推 GitHub、用 gh 读 Vercel；其余用数据目录里的副本（`site-sandbox/website`，APFS 复制）和本机的假远端（`site-sandbox/remote.git`），上线结果是模拟的；远端不是本机文件夹时一律不提交、不推送。
- **外部程序**：`/usr/bin/git`（推送靠钥匙串里已有的 git 凭据，要密码就报错、不卡住）；gh 只读上线结果；Node 按网站的 `.nvmrc` 在 nvm 里找。缺哪个只关掉用它的功能，写明原因。
- **本地服务**：`npm run dev` 在自己的进程组里跑（正式 4328，副本 4338），退出 Pomelo 时整组停掉；端口上已有别处启动的就直接用、不停它。启动时设 `ASTRO_DEV_BACKGROUND`，让 Astro 留在前台。预览是 Rust 开的 `site-preview` 窗口（不能调 Pomelo 的命令）。
- **发布**（`publish.rs`，后台任务，步骤见它开头）：fetch 只快进（分叉、冲突、不在 `main` 都停）→ 确认框带着改动的指纹 → `npm run qa`（最多 15 分钟）→ 再确认没落后、指纹还对才 `git add -A`、commit（文字原样）、`push origin main` → 每 8 秒用 `gh api` 读 Vercel 状态，15 分钟没结果写「没等到」。改动清单（`changes.rs`）是 `git status` 加没推的提交，按路径认出是哪类内容。
- **读写内容**（`yaml.rs`、`doc.rs`；各页 `wander.rs` `now.rs` `about.rs` `career.rs` `terms.rs`）：读用 `yaml-rust2`（键保持顺序）；写是 js-yaml 4.3 `dump` 的逐行移植（网站原有文件就是它写的），JSON 照 `JSON.stringify(…, null, 2)`。键原地改、不认识的原样留、没变的文件不写、写之前比对读进来时的指纹。85 组对照测试，另有只读测试和 Node 跑的 js-yaml 逐字比对。
- **资源库**（`images.rs`、`registry.rs`）：内容只写引用，保存时查引用的是资源库里的文件。加文件都走 `site_files_add`（大图片用 `sips` 转 JPEG、缩长边；网站别处有一模一样的就拒绝）。删之前照网站 `asset-reference-report.mjs` 的规则扫引用。界面经 `pomelo-site://localhost/<网址>` 显示网站里的图片（只认 `public/` 下的）。

## 有板有眼

- **Pomelo 不存画布**：`canvas.rs` 只记文件夹（设置 `canvas.folder`，默认 `~/Documents/GitHub/visual-atelier`）和上次看的那块（`canvas.last`）。
- **画布服务**是 visual-atelier 的 bench（`node bench/bin/dw.js serve --port N --json`，只听 127.0.0.1），Node 按它的 `.nvmrc` 在 nvm 里找（和呦呦漫游同一套 `site/tools.rs`、`site/server.rs`）。端口上有别处启动的就直接用；不是它（或是没有 `/api/status` 的旧服务）就说端口被占着。正式版用 bench 自己的 4321，开发版、测试、临时数据目录的打包版用 4331。Pomelo 启动的在退出时整组停掉。
- **接口**照它的 `bench/README.md`「给别的程序用」，用到的地址和各 `api` 版本多了什么见 `canvas.rs` 开头；旧服务缺的就不做（只有 `GET /api/canvases` 的：画布当 Explore Deck、页面当 Questionnaire；不能改名、删除的说「停止再启动」）。改名、删除、撤销（`canvas_rename`、`canvas_delete`、`canvas_name`、`canvas_restore`）照发，bench 不肯时把它的话原样给页面；删掉的进 visual-atelier 的回收处，Design System、Motion Deck 只改 bench 里显示的名字（记在那边的 `bench/names.json`）。批准、退回只在 bench 的界面里点。
- **页面**（`modules/canvas/`）：画布界面放在 frame 里（bench 的 `/#/<画布>?page=&board=`、`/#/ds/<方案>`、`/#/motion/<方案>`、`/#/qa/all`；问卷是 `/c/<名字>/`），左边点了用 `location.replace` 换地址（不留历史）。bench 发出来的消息 `{ source: "dw", type }`：`route`（带 `ds`、`motion`、`checks` 是在看哪类）左边跟着选中，`reviews` 换画板的批准标签，`changed` 重读清单。同一个名字可以既是 Design System 又是 Motion Deck（`pomelo`），所以左边按「类 + 名字」认。收起的类记在设置 `canvas.closed`。页面跨域取不到 bench 的 JSON，所以列表由 Rust 去问。

## Atlas

- **数据在 Rust**（`youzi/atlas.rs`）：年度总结的信写完，Rust 先存一份当时能读的数字和记录的快照（「呈现结果」），再让同一个模型写各幕的话、挑记录编号、归类聊天的话题，查过存进 `year_atlas`；这一步失败不影响信。每运行一次多一行，就是历史。补做、立即运行都走年度总结那个定时任务的流程和模型（`schedule.rs` 的 `atlas_run`）。
- **画在设计里**：设计是同步来的冻结快照（`src/vendor/pomelo/atlas/<名字>-<版本>/design.js`）。Rust 的 `DESIGN` 是新写的那一份记的设计；每一行记着自己的，打开时由 `modules/atlas/designs.ts` 找来画（打开小报时才加载；找哪一份见 `design.md`「Atlas」）。
- **小报放在 frame 里**（`Journey.tsx`）：frame 的文档是快照自己带的（样式、上墨的脚本都是冻结那天的），里外的样式互不影响，旧的小报不会被后来的界面改动弄坏。同一个来源，页面直接把数据交给它，它把「在模块里打开」「收进抽屉」「Esc」告诉页面。
- **模型写的只有话**（几幕、各写什么见 `requirements.md`「Atlas（年度总结）」）：数字、日期、图、泡泡都不归它，它写的 Rust 都照快照查一遍，不认识的丢掉。回复照宽了读（`parse`），读不出来告诉它原因再要，一共三次（`ask`）；关不掉思考的云端模型（GLM、GPT）每次都留足思考的余量，后两次让它少想。
- **信存两份**：对话里、`year_atlas.letter`；明年写信带的「去年的年度总结」和留对话关着时都读后者（`yearly.rs` 的 `letter_of`）。
- **各幕的数据**都在 `data.rs`，和年度总结读的同一份：自建模块 `year_made`、玩物有志 `year_games`、见字如面「写下的字」`year_writing`。自建模块写完各幕再问一次模型怎么排（`youzi/made.rs` 的 `arrange`，照 `SKILL`），Rust 照 `LAYOUTS` / `FITS` 核对、填数（`fill`）。「重要的时刻」抄的句子（`quotes`）Rust 对着正文查，文章正文只在这一步读进内存。
- **删除只在页上**（`atlas_delete`，进撤销）。首页的信封由 `atlas.rs` 的 `envelope` 决定，「那年今日」在 `home.rs`。

## 系统通知

- **规则都在 `notify.rs`**：设置 `notify.*`（不进撤销，键见 `schema.md`）。待办、投递、来日方长由柚子的后台循环每 30 秒看一次：到了时间、今天还没看过就算一遍、发一条，今天记进 `notify.sent`（没东西可说也记）。算的和页面用同一套函数（`todo::agenda`、`jobs::jobs`、`days::list`）。定时任务在 `schedule.rs` 跑完时发。
- **点通知**：通知带着要打开的东西（`userInfo` 的 `open`），点了叫出主窗口，发和速记小窗一样的事件（`capture-open`、`capture-chat`）。
- **只有真的才弹**（UserNotifications 只能在 .app 里用）：其余只把要发的写到终端。临时数据目录的打包版带 `POMELO_REAL_NOTIFY=1` 能试真的（和正式版同一个 App，系统的允许共用）。

## 首页、月历和每天的例行检查

- **首页、月历**：数字和清单由 `home.rs`、`calendar.rs` 每次打开时现算（只读，不存），隐藏的模块不算；今天的待办直接用一步一步的列表。
- **例行检查**（`daily_sweep`）：每天第一次打开、或开着过了零点：每日备份；把时间段结束或已打满的打卡任务标为完成；记上到日子的固定支出（漏掉的一起补）。窗口关着、只留菜单栏时，每日备份每 5 分钟查一次，另两件等下次打开窗口补。不进撤销（撤销了下次还会再做）。

## 图表

都在 `src/shared/charts.tsx`，自己用 SVG 画，不引入图表库；有哪几种、样子和颜色见 `design.md`。

## 占用空间和内存

App 本体约 25 MB，运行时约 130 MB 内存；`pomelo.db` 很小（十年也在几百 MB 以内），真正占空间的是附件。

## 柚子（AI）

- **旧名**（名字的规矩见 `CLAUDE.md`「柚子（AI）」）：读旧数据还认 picky：数据文件 38 之前的 `picky`、旧文件夹名（`youzi/store.rs` 的 `adopt_old_names`）、旧偏好键（migration 0038）；聊天记录里的「小拾」不动。
- **柚子服务**（`provider.rs`）：统一的「发消息、流式收回复」，背后是本地模型（8080，OpenAI 兼容）或六家云端（固定的官方 HTTPS 地址；Anthropic 用 Messages 接口，其余是 OpenAI 那种）。回复、思考、用量边收边交给界面，定时存进 `youzi.db`，中断时存下已收到的；出错只显示固定的中文提示。每个对话一个生成位置（`chat.rs` 的 `Runs`）；本地模型 `--parallel 1`、请求在这里排队，云端不排队。云端还没收到内容时遇到 429、5xx、流里的错误或连不上，等 2 / 5 / 10 秒（照 `Retry-After`，最多 20 秒）重发，最多 3 次。
- **思考**（调用时的规矩见 `CLAUDE.md`）：只有「能开关」的模型收到开关；模型不收某个设置就去掉再发一次，记住到退出；阿里云内容审核拦下时直说、不重试。查数据的那几轮按各家要求原样发回思考内容（签名等），以前的问答只发文字。**强度**（`level_for`，在 `stream` 开头）：请求选的模型有就用，否则用模型固定的默认，再没有就不发；高强度时在短的回复上限上加余量（`Options.room_fixed`：调用方算过就不加）；词原样发，400 就去掉再问并记住（`模型#词`）。存「开关 + 强度」的：`sessions`、`schedules`、偏好 `brief.effort`、`assistant.translate`、`rater.options`、Seeker 的 `options`；存 `cloud::Level` 的：Pub2MD、Importer、面板帮手（`assist.model`、`assist.level`）。选项由 `Thinking.tsx` 按当前模型列（助手设置页是 `ModelRow.tsx`）。中间步骤（对话压缩、读长文件、年度总结的每月阅读）用模型的默认。
- **本地模型**：读 `~/Documents/LLM/config/models.json`（聊天、向量、重排），`llmctl.sh` 启停，`/health` 看状态；聊天模型同时一个（8080），向量（8081）、重排（8082）用时自动启动；柚子启动的闲置 30 分钟后停、退出时停，别处启动的不碰。只有界面和菜单栏能控制。
- **读文件**（`files.rs`、`office.rs`、`reading.rs`；各格式怎么读见 `office.rs` 开头）：添加时读一次存成分段文字；大表格只存表头和前 50 行（「查表」从副本现读）；网页在 Rust 里下载（20 秒、5 MB 以内）；图片发云端前在临时文件夹里用 `sips` 缩小。没有文字的 PDF、只有图片的 Word / PowerPoint 当附件时存一句「没有文字、共几页 / 几张图」（`prepare`；知识库里仍当扫描件），云端模型用读文件的 `look` 看。
- **看文档里的图片**：Word、PowerPoint 从 zip 里取图；PDF 用 Quartz 的 PDF 对象数每页的图（`pdf.rs` 的 `image_pages`，PDFKit 不给），看时用 PDFKit 画整页。`chat.rs` 的 `file_images` 和 `knowledge.rs` 都在数据库锁外读、只给云端、一个回答共用 8 张，图片跟在这一轮的结果后面发。
- **挑选和检索**：记忆和资料段落先按向量相似度取一批（记忆 12 条、资料 20 段，0.30 以上），再用重排挑至多 6 条；重排用不了只按相似度（0.35 以上）。知识库同样。**出处**（`cite.rs`）：一次回复拿到的统一编号（同一条记录一个号），随回复存进 `youzi.db`；「读整条」读前再按规则查一遍。
- **查数据**（`data.rs`）：模型的工具调用；Rust 先查能不能读、能不能发云端，再查、截短交回，查到的存进 `youzi.db`。钱来钱往的云端按两块算（`cloud_key`：资产 = 账户、余额、持仓，其余是收支），查询、页面、卡片、搜索结果各按自己那块挡。
- **改数据**（`act/`，细则在 `act/mod.rs` 开头）：七个模块共 14 个工具；用模块自己的函数在撤回的事务里试写，前后一比就是卡片，告诉模型的话照抄卡片。查数据的 4 轮后还有一轮只给写数据的工具。卡片每条有编号（第几条回复的第几条），前面没确认的卡片列在说明里（只列这个模型能看的模块），`replace` 只限当前显示的分支。
- **问我**（`ask.rs`，细则在它开头）：工具 `pomelo_ask_user`，卡片存进 `run_asks`，后台任务停在 `ask::Gate` 上，回答变成工具结果接着走，期间生成位置一直占着。退出后的回答当新消息发，所以不存模型的中间状态。
- **面板帮手**（润色、补全、一句话填好 / 改好，`assist.rs`）：一问一答。Rust 写好提示（填好附上今天前后的日历和已有的分类、分组、渠道、模板；改好再附上 `data.rs` 的 `record_text`，界面只传编号），回答读成字段再查一遍才交给面板（日期真实、名字对上、重复规则 `todo.rs` 的 `Rule` 能读、链接是句子里写了的；改好不补默认值，`clear` 只收能清空的几项）。呦呦漫游在 `data.rs` 里「只帮忙、不读」（`HELP_ONLY`）。
- **插件库**（`skills/`）：开着、Key 齐了、并且「自己决定用」或这一条点名要它才给（点名的在系统消息里写明必须先用；不用 `tool_choice: required`，有的服务不认）。查到的都编号成出处。联网（`web.rs`）调 Tavily 的固定地址；书单 / 影单检索（`works.rs`）的 Wikidata 排队、每次隔 0.35 秒，请求头带联系邮箱，查到的在内存留 6 小时。读文件、查知识库由 `chat.rs` 执行，放不下时发每个文件的目录和按问题挑的段落（最多 8 段，每个文件至少第一段）。
- **定时**（`schedule.rs`）：后台循环每 30 秒看一次到点的，和发消息走同一条路，只是只读、系统消息说我不在；要用的本地模型没开就先启动，跑完只停自己启动的。「上次跑到哪」不进撤销。工具（`chat.rs` 的 `read_only`、`lookup_plan`）：不给写 Pomelo 数据的和题库，给工具箱（卡片按 `run_id` 存），联网搜索开着、有 Key 就给（`Ctx.scheduled`）。
- **今日简报**（`brief.rs`，细则在它开头）：Rust 先用 `data.rs` 读好要用的（每块有上限），发一次请求：系统消息是偏好 `brief.prompt`（空就用 `DEFAULT_PROMPT`），日期和数据在用户消息里；最多等 5 分钟，不占对话的生成位置。
- **年度总结**（`yearly.rs`，细则在它开头）：Rust 先读好全年和算好的数字；放不下就按月整理要点（`month_summaries`，没变就直接用）再写全年；写的哪一年记在 `year_runs`。写完照「留对话」删不删对话（`schedule.rs` 的 `drop_chat`：只删写成的）。「定时」里的流程图是 `yearly.rs` 的 `FLOW`（和插件库的 `Flow` 同一种），改流程要一起改。
- **知识库**（`knowledge.rs`，细则在它开头）：`youzi.db` 只记库、来源、开关这类选择（能撤销）；文件清单、分段和向量在 `search.db`（后台扫描只写它，不挡 ⌘Z）。不监听文件，定时比大小和修改时间；建索引一次一个，文字没变的段落沿用旧向量。「打开文件」先查路径没出文件夹。
- **助手**（`assistants/`）：设置和上一次的结果在偏好 `assistant.<名字>`，不进撤销和导出。**Translator**（`translate.rs`）：长文按段切块依次流式翻，后一块带上前一块译文的结尾（300 字以内）；在后台任务里跑，结果每秒存一次、页面轮询。**呦呦漫游 · 按中文起草**（`site.rs`）用同一套 `translate_with`，结果只在内存。
- **Pub2MD**（`assistants/pub2md/`，流程在 `job.rs` 开头）：直连 Mathpix，每步存进 `.pub2md/job.json`（「继续」从那里接着做）；`mmd.rs`、`chunk.rs` 切块，`check.rs` 只比文字，`prompts.rs` 修模型常出的小毛病；联网查术语用各家自带的搜索（`search.rs`），其余用 Tavily；简繁用 `script.rs`（ferrous-opencc）；预览用 mathpix-markdown-it，放进不能跑脚本的 iframe。页面命令都是 `async`，排队的只在内存；模拟版用 PDFKit 读出的文字代替 Mathpix。
- **Work**（`work.rs`、`word.rs`、`skills/toolbox.rs`，细则在 `word.rs` 开头）：.docx 自己生成。改文件只改 Rust 列出的文件（附件的原文件、旧问题选的文件、知识库里的），提议逐段查过放进卡片（一条回复一个文件，记下指纹），写入时文件变了就不写；Word 只改变了的字，其余原样，改不了的拒绝。
- **Excel 工具箱**（`excel.rs`，细则在它开头）：不加新库（zip + quick-xml；calamine 只读）。只重写动到的表；加删行时公式、合并格这些跟着挪，挪不好的拒绝；写入时要求打开时全部重算。和 Excel 不同的一处：加在范围最后一行后面、公式在这几行外面时，范围扩到新行（`grows`）。读大文件在锁外（`excel_plan`，再 `excel_commit` 放进卡片）。
- **DevJanitor**（`assistants/janitor/`，规则见 `CLAUDE.md`「DevJanitor」，细则在各文件开头）：
  - **假 Mac**（`fake.rs`）：大文件是稀疏文件，`.janitor-fake.json` 写着假的磁盘大小、在跑的程序、Docker；删除和废纸篓都在里面，命令不真跑。
  - **扫描**（`scan.rs`）：8 个线程走一遍，只看名字、大小、时间；真磁盘约 135 万个文件、17 秒。结果一次整个换进 `janitor.db`，清理后从各层减掉腾出的大小。开发环境（`env.rs`）的大小从扫描的树取，每个环境认领自己的文件夹。**问系统**（`system.rs`，不加依赖）：`df`、`ps`、`lsof`（「在用」，删之前再看一次）、`plutil`、`mdls`、`docker system df`、`sysctl`、`vm_stat`；完全磁盘访问权限看 TCC.db 能不能打开（不弹框）。
  - **Agent**（`agent.rs`）：借柚子的对话（私密，偏好 `janitor.chat`）和生成的路，换成 DevJanitor 的模型、SKILL 和工具（不给柚儿园的数据），最多查 12 轮；只读命令过了 `readonly.rs` 的清单才用 `zsh -lic` 跑（60 秒停）。卡片在锁外查好（路径、大小、受保护、sudo、规则范围）存进 `janitor_cards`、边写边发。
  - **受保护**（`rules.rs` 的 `Protected`）：一键清理认条目时跳过、清理前再查；只列出来的条目页面确认两次才移走。页面只传条目编号，路径从 `janitor.db` 取。柚子查它用只读工具 `pomelo_janitor_info`，和 Agent 看的同一份文字。
- **体检**（`health.rs`）：只读、现算，结果用全局搜索的 `Hit`，页面照 ⌘K 打开。**空间管理**：算数据库和 `youzi-files/` 的大小，列出每份副本被谁用着；删副本只删文件、留记录（「文件已删除」）。
- **云端 Key**：界面只显示「已保存」；App 打开时读一次钥匙串那一项、放在内存，查「有没有存」从不弹框；一个 Key 一项的旧存法打开时搬进来再删。
- **开机自动启动**（`autostart.rs`）：Tauri 的 autostart 插件写一个带 `--hidden` 的 LaunchAgent（不用 AppleScript），这样打开时窗口第一次画出来前就藏起来；每次打开让登录项跟着设置里的开关（默认开）。
- **Seeker**（`assistants/seeker.rs`、`seeker/`，细则在 `seeker.rs` 开头）：打字停一下自动存（`seekerDraft.ts` 排队，结构操作前先存完），一次保存一步撤销。导入只读 `transfer/` 认得的 Markdown，按来源（`文件#标题:第几个`）对上以前导入的。发给模型的资料在 `model.rs` 的 `context` 现拼，规则「标题 + 分点」（`catalog.rs` 的 `rules_markdown`，Rater 也用）。模型每次只调一次、没有工具，按功能选（`catalog.rs` 的 `Settings::of`）；发给模型的草稿带短编号（`outline`），回答按编号查过才给差异或变成批注（`notes_from`，调用结束时 Rust 自己存进草稿），对不上的略过。页面的调用在 `seekerJobs.tsx`（不跟页面卸载）。开发版打不开真的 Seeker 文件夹（`output.rs` 的 `allowed_path`）。岗位关联、「做简历」、公司资料都先读 `pomelo.db` 再锁 `youzi.db`，不嵌套。
  - **Word**（`word.rs`、`seeker/output.rs`）：模板编进 App（`src-tauri/templates/`，`scripts/resume-templates.ts` 生成，只留格式，生成后查一遍没有个人文字），样板段落的位置写死在 `SLOTS_CN` / `SLOTS_EN`；照样板段落的格式一段段写，纸张、样式、编号原样，去掉缩略图；不数页数，只新建文件。
  - **公司资料**（`seeker/company.rs`）：`knowledge/` 加进知识库「Seeker」后原地读；选材、审查时用 `knowledge::retrieve_under` 按 JD 取至多 6 段放进系统提示。cv/、material/（`seeker/files.rs`）每次打开页面现扫、不存库。岗位用的简历按文件路径记在 `youzi.db`；投石问路旧的「简历」一栏开机时搬一次（`adopt_job_resumes`）。每个命令带语言、只动那一种。
  - **规则发给谁**只写一处（Seeker `model.rs` 的 `reads` / `task_of`，Rater `qbank::reads`、`practice::READS`、`interview::PREP_READS` / `SUMMARY_READS`、`record::READS`），调用和规则页的「发给」「发给模型的说明」都用它（`previews`）；说明开头是 `model.rs` 的 `PRINCIPLES`。自带的组改名只在 `seeker_groups` 记显示名，自己建的组改条目的类别。资料和题的顺序是 `sort` 列（照「拖拽排序」），组的先后在 `seeker_groups`。
  - **柚子读 Seeker**（`skills/seeker.rs` 的 seeker-toolkit）：按这条回答的权限挡：Seeker、Rater 各看隐藏，云端都看 Seeker 的「允许发给云端」（`chat.rs` 的 `access_for`）。
- **Rater**（`assistants/rater.rs`、`rater/`，细则在 `rater.rs` 开头）：题目一条一行 JSON，保存按版本号查、旧的进历史、一步撤销；秒数读时算。程序自带的规则（`qbank.rs` 的 `builtins`）组里没有能顶替的就加上（`group_rules`）。模型和思考按功能选（`qbank.rs` 的 `Options`、`chosen_model`），存在偏好。
  - **三个题库 SKILL**（`rater/qbank.rs`，对话里 `skills/qbank.rs`）：只调一次模型、没有工具；模型回 JSON，Rust 按编号查素材、改自哪道、去重，再交给页面或确认卡片（`act/card.rs` 的 `add_rater`，确认时 Pomelo 的记录先写）；`replace` 用同一套 SKILL 重写答案。
  - **卡片练习**（`practice.rs`）：一轮一行 JSON，每个动作一步撤销（点评也进，否则撤销前面的会被挡住）。追问先查能不能追问、调完再查一遍才写。
  - **面试**（`interview.rs`）：准备、复盘、总结各一行 JSON，挂在岗位上（编号 + 创建时间）；Rust 只留题库里真有的题号；总结把几次复盘的问题接着编号，模型写「问 N」、Rust 换回是哪次的第几个。总结在 `seekerJobs.tsx` 里跑。
  - **复盘 · 记录**（`record.rs`）：开始时建一个私密、知识库打开的对话和一行 `rater_records`，这个对话在各处的 SQL 里排除（见 `schema.md`）。`chat.rs` 的 `prepare` 改用它的模型和思考，`generate` 接上任务、现在的表、JD、速查卡、以前几轮和规则，多一个 `pomelo_review_fill`（每次写整张表）；页面回复期间每 1.2 秒读一次表。存成复盘一步撤销；聊天和填表不进撤销。

## 见字如面的导出（PDF、Word）

- **一个隐藏的窗口**（`print.rs`，页面 `print.html`）：导出时 Rust 开它，交给它这次的篇目、存到哪、样式，做完关掉；一次只导一批。
- **排版用编辑器自己的画法**（`bear.ts` 的 `drawDoc`，不带光标、不折叠、记号都藏起来），所以 Bear 的写法在 PDF 里和 App 里一样。按行切成 A4 页（1 CSS 像素 = 1 磅）：标题不留在页底，空行不放页首，图片、公式、流程图缩到一页放得下。
- **PDF**：页面一次只露一页在窗口左上角，Rust 让 WebKit 把这块画成一页 PDF（`createPDF`，字还是字），再用 PDFKit 合成一份，加上目录的跳转、网址链接、书签和标题。
- **Word**：页面把排好的每一行拆成段落和文字片段（加粗、高亮、链接……）交给 Rust（`articles/docx.rs`），公式和流程图先画成图片；Rust 写出 .docx（样式、目录域、页眉页脚、图片）。
- Markdown 不经过这个窗口：Rust 直接拷正文和附件。
