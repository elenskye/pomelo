# 验证方法

> 这里只写怎么测：什么时候跑什么、命令和参数、各块要点名的测试；什么先问我、哪些绝不碰见 CLAUDE.md，模拟的和真的怎么分见 `architecture.md`「开发版和正式版」，各处该怎么样见 `requirements.md`。Rust 测试都是 `cargo test --manifest-path src-tauri/Cargo.toml <名字>`，下面只写名字（写模块名就跑那一块的全部）；`real_`、`replay_` 开头的要加 `-- --ignored --nocapture`。

## 什么时候跑什么

| 什么时候 | 做什么 |
|---|---|
| 改完任何代码 | `pnpm check`（样式不串 + 类型检查 + 双指滑动的模拟 + 写字的框的测试台（无头 Chrome）+ Rust 测试） |
| 改了写字的框（`shared/editor/`、`bear.ts`）或它们的样式 | `node scripts/editor-lab.mjs --safari`：同一个测试台在 Safari（WebKit）里再跑一遍 |
| 改了见字如面的双指滑动（`swipe.ts`） | `node scripts/swipe-sim.mjs` 要 0 种不对（`… 7 all` 换一组随机数并列出每一种） |
| 改了组件 | `node scripts/hooks-lint.mjs`：hook 写在提前返回后面的要 0 处 |
| 改了界面、手画零件、外框 | `node scripts/ui-lint.mjs`（「界面检查」） |
| 改了外框、纸、手画零件、动效 | 在 Safari 里量流畅度（「WebKit 才有的问题」） |
| 新加或改了 Rust 命令的页面 | `?mock` 之外，在打包版的真实窗口里点一次（「真实窗口」） |
| 装正式版前 | 升级测试 |

## 界面：`?mock`

- 用 Bash 后台跑 `pnpm dev`（预览面板没有「文稿」权限，起不来），再用 `.claude/launch.json` 的 `ui-mock` 开 `http://localhost:1420/?mock`（内存里的模拟数据，不走 Rust），某页用 `/?mock#/finance`。改仓库里任何文件（文档也算）页面都会刷新、模拟数据清空，等结果时别动文件。
- 截图常慢一拍，关键结果用 JS 读页面（下拉的选项是 `[popover] [data-index]`；模型菜单按服务折叠，先点分组行）；拖拽用 `left_click_drag`，勾选用带 modifiers 的点击。
- **地址参数**：
  - `&vault=new|open`（默认锁着，测试主密码 `mock-master-1`）、`&touch=fail|none`。
  - `&youzi=ready`：一打开就有模拟的本地模型，「模板」有置顶模板。
  - `&kb=demo`：先放两个知识库（一个扫描件、一个读不了的文件）；打开知识库后扫描件走一遍本机识别（准备识别、识别中，能停下、接着识别）。
  - PDF 工具箱：问「合同 PDF 里押金那段在哪一页」，回答引用 PDF 的一页，点开在卡片里看那一页（找到的字画出来）。
  - `&brief=fail|off`：今日简报；`&mood=calm|busy|good|worry|plain` 换简报的心情，柚子跟着变。
  - `&atlas=envelope`：假装是新年第一周，首页有信封。
  - `&site=sandbox|missing|clean|big|ahead`：呦呦漫游的开发版还没副本、找不到文件夹、没有改动、有超大的文件、有没推的提交。
  - `&pub2md=keys`：像正式版那样要填 Mathpix 的 Key。
  - `&finance=images|dbs`：导入账单拿到截图或 DBS 的 PDF。
  - `&health=clean`（体检什么都没有）、`&canvas=missing`（有板有眼的文件夹不在）。
- **模拟的柚子**：问题里写「滚动测试」「失败测试」得到长回复 / 失败。提议写数据后，下一句「《X》是Y写的」换掉前面卡片里那本书的那一条（Rust：`youzi::act`、`a_follow_up_changes_the_earlier_replys_pending_card`）。选文件：第一次拿到 pdf、md，第二次多选拿到 xlsx、pptx、png。
- **出错兜底**（只在开发版）：`window.__pomeloCrash("念念不忘")`（页的名字，或「首页」「贴纸簿」「柚子瓣」「找东西」「问柚子」）让那一块画的时候抛一次错，看兜底的卡（`design.md`「外框 › 出错兜底」），顶栏和别的页照常。

## 界面检查

`pnpm dev` 开着时 `node scripts/ui-lint.mjs`（无头 Chrome，用 visual-atelier 的 playwright-core，那边 `npm ci` 过就行；跑几分钟）：每一页和几个编辑面板在 1440 × 900、1180 × 780 里走一遍，按类列出 board（白板）、plain（浏览器画的直线框，小的附件签、文件名签不算）、frame-*（手画的边在会滚的盒子里或被切掉）、covers-frame（涂了底色的行压着手画的边）、text-out / child-out（字或格子出框）、wheel-trap（会接走滚轮的 `overflow: hidden`，Chrome 里多半试不出来，靠它拦）、BLANK（整页白了）。

- 后面的参数换配色和明暗（`… lint.json 1440x900 blue dark`），`ROUTES=/finance,/todo` 只查几页，`SHOTS=<文件夹>` 存编辑面板的截图。
- 故意保留的细边框加 `data-nolint`。首页问候的荧光笔会报一处（字形比行高高一点，不是出框）。

## WebKit 才有的问题

窗口是 WebKit，有些东西只在它里面慢或出错，Chrome 和内置浏览器里看不出来。

- **流畅度**：`pnpm dev` 开着时 `node scripts/perf-listen.mjs 1499 <结果.json>` 等结果，再 `open -a Safari "http://localhost:1420/?mock&perf=1499"`（`src/dev/perf.ts`）：页面自己开十个页、在开着的页之间换、各滚一遍，打出帧间隔的表。Safari 要在最前面、屏幕没锁，否则不出帧。`&perfcss=<样式>` 先塞一段样式再量（比如藏起某一层对照）。一帧超过 100 毫秒就要查（`design.md`「动效 › 流畅度」）。
- **在 Safari 里遥控**：`open -g -a Safari "http://localhost:1420/?mock&drive=1498"`（`-g` 不抢前台），再 `node scripts/drive.mjs 1498 <脚本.js>`：页面（`src/dev/drive.ts`）把脚本当 async 函数体跑，打出 `return` 的东西；脚本里有 `sleep`、`$`、`$$`、`type(输入框, 字)`、`key(键)`。后台标签页不跑动画和 `requestAnimationFrame`，只能量排版和状态，过几秒还会被挂起（只跑得动一段，等的时间压短）。开发版的真窗口：`VITE_DRIVE=1498 pnpm app:dev`。靠真鼠标、键盘焦点才出现的毛病（比如有焦点的输入框滑出窗口）要在打包版里用电脑操作、全屏控制。
- **见字如面**：`?mock#/articles` 有编的十来篇（「Rust 生命周期笔记」用上每种格式；一篇加密的，`&vault=open` 时能看；一篇在废纸篓、一篇归档）。Rust：`modules::articles`（含搬家、导入、导出）、`modules::tags`。
  - 记号露出时是 `.fx.on`；在格式前、格式里打字，那一行的节点不换，在一段里打字 `.ar-text` 的子节点一个都不换；提示框开头的图标是 `.co-head svg`，只有记号的 `> [!TIP]` 行是 `.cohid`。右键的纸、格式条「更多格式」的「提示框」：对着一行发 `mouseover` 或点它，旁边出 `.md-sub`（右边放不下在左边）；图标、颜色点和字之间 8 像素。
  - 双指滑动：脚本发 `WheelEvent`（`deltaX` 先变大再慢慢变小），往左一下收一栏，滑行里接着来一下也算；发几个后用空循环卡住页面 200 多毫秒、再发一个位移很大的，也只收一栏。列表的一条左滑露出删除时 `.ar-swipe` 的 `data-off` 停在 76；鼠标拖用 `PointerEvent`（`pointerType: 'mouse'`）。列表的一条、左栏的标签右键发 `contextmenu`。
  - 只能在打包版里看：触控板的手感、新写一篇时光标的位置、**输入法**（脚本发的 composition 事件和真的不是一条路）。用电脑操作的全屏控制（要我同意）在临时数据目录的打包版里打拼音选字：标签下新写一篇在空标题上打（不能和下一行的标签并成一行）、段落末尾回车后接着打（不能多出空行）、标题和段落末尾、列表里；停在候选框上一秒多（会保存和刷新）再选字，字不丢、光标不跳。Safari 里能用脚本模拟 WebKit 把字放错地方（发 compositionstart、改掉 DOM、发带 data 的 compositionend 和 input），结果应是原文在开始的位置插进那几个字。
  - 搬家：`real_bear_import` 只读地核对真的 Bear（不设 `POMELO_BEAR_INTO` 时搬进内存里的库和临时文件夹，逐篇比时间和图片；设了就搬进那个数据目录，先留「导入前」备份，App 要关着）。
  - 导出：PDF 的样子在浏览器里看 `/print.html?mock&id=4&style=pomelo|print&toc&header&meta&repeat=3`（`repeat` 把正文多来几遍看分页；`window.__laid` 是页数、链接和书签；加 `&word` 看拆给 Word 的块）。真的 PDF / Word 只能在打包版里出：临时数据目录打开时带 `POMELO_TRY_EXPORT='{"target":{"all":true},"options":{"format":"pdf","style":"pomelo","toc":true,"header":true,"meta":true,"dest":"<临时文件夹>"}}'`，打开 5 秒后导出一次，结果写在数据目录的 `export-try.json`（只认临时数据目录）。
- **柚子的右键和归档**：`?mock#/youzi` 对话、项目的文件夹、消息右键发 `contextmenu`（出什么见 `requirements.md`「聊天」；生成时不出保存为 Word、重新生成）。点「归档」后列表最下面多「归档 1」；`?mock#/settings/youzi`「归档的对话」点「在柚子里看」，到柚子、「归档」展开在眼前。
- **柚子链接**：`?mock#/notes` 输入框里打 `[[示例`，提示 `[role=listbox]` 里混着游戏、书，↩ 写成 `[[玩物有志/示例游戏 A]]`，记下后是小卡 `.pm-lk`（颜色 `--pm-link-pomelo`），面板最下面「柚子链接 · 1」。`?mock#/articles` 正文：单独一段的 `![[念念不忘/…]]` 是 `.emb-card`（目标用 `links_suggest` 取一条笔记的 `target`）；选一段右键，「记到念念不忘」弹 `.pm-clip`，「柚子链接…」弹 `.pm-linkpick`，点一行选中的字变成 `.pm-lkw`；被链到的那篇右上 `.pm-bkb`、文末 `.pm-bkx`；右上的羽毛开书架 `.ar-shelf`，用 `PointerEvent` 把小卡拖到一段后面，多一张 `.emb-card`。书影游面板的「记一条」、五花八门「游戏」清单的「⋯ → 搬到玩物有志…」（确认后到 `#/games`）也查一遍。Rust：`modules::links`、`modules::games`。
- **写字的框的测试台**（`node scripts/editor-lab.mjs [--safari]`，自己起一个 Vite，不占 1420）：在真页面里把编的样本用三种写法（见字如面、念念不忘、写提示词）各画一遍，查字叠、样式串、读回的字和光标、局部重画、能不能点（细则在 `src/dev/edlab.ts` 开头）。新的写法、新的记号先加进样本。
- **样式不串**：`node scripts/css-lint.mjs`（在 `pnpm check` 里），查 CLAUDE.md「界面规则」的「样式只管自己」。
- **写字的框**（右键的那条纸、字里的图标，该怎么样见 `requirements.md`「图标库、写字的框」）：`?mock#/notes` 输入框里右键，点格式、点两个图标库；Markdown 的在 `?mock#/site` 的作品正文或写提示词的框里。预览面板和后台标签页没有键盘焦点，`[[`、`#` 的提示出不来：先对着输入框发 `new FocusEvent('focusin', { bubbles: true })`。速记小窗（`capture.html?mock`，520 × 320）里提示列表不出窗口、不盖住在写的那一行，放不下就在里面滚。光标和退格碰到字里的图标各家浏览器不一样：随手起一个收 POST 的（`node -e` 写文件就行）听一个端口，再 `open -g -a Safari "http://localhost:1420/?mock&edtest=<端口>"`（`src/dev/edtest.ts`；页面标题变成 `edtest ok` / `edtest FAIL`，`window.__edtest` 里是每一项），要 24 项全过。Rust：`notes`（一行预览去掉图标）。
- **落签**（该怎么挂见 `design.md`「外框 › 落签」）：挂出来的是 `.tip-float`，靠纸边的按钮（`?mock#/site` 右上的「刷新」）纸签要整张看得到；`.tip-float .pm-tip-string` 的 top 比 `inkBottom` 量出的最低一笔低 3 像素左右，线的中线对着按钮的中线。`?mock` 里 `(await import('/src/shared/tips.ts')).inkBottom(按钮, null)` 直接量，窗口 1900 宽（格式条不收起）。要量：时间穿梭、顶栏的找东西和设置、格式条的每个按钮（「引用」「柚子链接」「专心写」量整幅图）、「桌上开着」的每一页、叠着的页（开三四个模块再回一页，歪完 0.35 秒再量）、柚子瓣（含念念不忘那片叶子）。等纸签晃完（1.1 秒）再量，第一帧往上提了 8 像素。内置浏览器的面板藏起来时不走帧，用无头 Chrome；Safari 后台标签页先塞 `*{animation:none!important;transition:none!important}`。

## 真实窗口

- **打开**：`pnpm app:build` 后 `open -n --env POMELO_DATA_DIR=<临时目录> "src-tauri/target/release/bundle/macos/Pomelo OS.app"`，用 `screencapture -x` 截全屏看；查数据用 `sqlite3` 看临时目录。要一打开就停在某一页：先在临时目录的库里写 `INSERT INTO settings (key, value) VALUES ('dev.open', '"/canvas"')`，只管这一次启动，用过就自己清掉。
- **截图**：只有壁纸、没有窗口，是跑命令的程序没有「屏幕录制」权限：改在 Safari 里遥控查排版，或者我同意时用电脑操作截图。我在用别的 App 时，新窗口可能被台前调度缩成左边的缩略图，不是 bug，别为了截图把它拉到前面。红黄绿按截图量：圆心在 `trafficLightPosition` 的 y − 2 处，按钮之间约 23。
- **点**：交互在浏览器里测；要在真实窗口里点，用电脑操作（不用 osascript，见 CLAUDE.md；授权框由我同意），只授权「Pomelo OS」，点之前确认正式版没在运行。网页视图在后台点不动，要全屏控制；文字框有焦点时，按下松开没有间隔的点击会被吞掉，按「按下、停 0.15 秒、松开」点。
- **收尾**：`pkill -f "pomelo/src-tauri/target/release/bundle"`（带上 pomelo 这一段：别的项目也有 `target/release/bundle`；别用 osascript quit，会连正式版一起关），删掉这个 `.app`。

## 升级测试（装正式版前做）

- 先在正式版二进制里 `grep -c -a` 各 migration 的特征 SQL，确定它在哪个版本（不读真实数据目录；`youzi.db` 的升级脚本也要查）。
- 在临时目录里用 `sqlite3` 依次执行到那个版本的 migration、`PRAGMA user_version=<版本>`、插几条会被新 migration 改动的数据，再照「真实窗口」打开新打包的 App，检查版本号、`backups/` 里的「升级前」备份和数据。
- 没有新 migration 时（只改了数据文件格式或记录的 JSON 形状）：确认二进制里已有全部 migration，用单元测试读旧形状的记录，再用临时目录打开新 App，确认能启动、版本号不变、数据没被改。

## 外框和首页

- **贴纸簿、找东西、勾待办**：点顶栏「＋」再按 Esc，`document.querySelector('.topbar').getBoundingClientRect().top` 还是 0；找东西打一个常见的字（`a`），`.sres` 的 `scrollHeight` 比 `clientHeight` 大，700 毫秒后 `.srow.pm-print-row` 一个不剩；`?mock#/todo` 勾一件，勾选框马上是 `aria-checked="true"`、带 `is-pop`，这一行 0.6 秒后才挪进「已完成」。
- **设置的五页**：`?mock#/settings/general`、`/data`、`/youzi`、`/modules`、`/health`，纸签是 `.ftabs [role=tab]`；`#/settings` 到上次看的那一页（第一次是通用）。
- **外观**：`?mock#/settings/general` 最上面「外观」，改了马上换（JS 读 `document.documentElement.dataset.palette` / `.theme`）；存在 `appearance` 设置里，localStorage 也留一份（`pomelo.appearance`），清掉它刷新能看到开窗时先用哪份。
- **首页和月历**：`?mock#/`、`?mock#/calendar`。Rust：`home`、`calendar`。
- **图标库**：`?mock#/todo` 新分组 / 编辑分组、目标，点名字前那一格；`?mock#/notes` 点标签前的图标。Rust：`todo`、`transfer`。
- **速记小窗**：`http://localhost:1420/capture.html?mock`（浏览器里拉到 520 × 292 看；模拟数据和主页面各是各的）。`window.__TAURI_INTERNALS__.invoke('plugin:event|emit', { event: 'capture-blur' })` 测点别处收起；主页面发 `app-menu`（`new-tab`、`close-tab`、`next-tab`、`prev-tab`、`go:0`～`go:8`、`settings`）测菜单的快捷键，发 `capture-chat`（对话编号）、`capture-open`（`{ path, hit }`）测从小窗打开对话和记录。问柚子、翻译加 `&youzi=ready`。「存为 .md」在 mock 里只回一个文件名，格式转换用 `node --input-type=module -e` 直接调 `src/shared/editor/lite.ts` 的 `liteMarkdown` 看。Rust：`capture`。⌥Space、盖在全屏 App 上、Esc 回到原来的 App、窗口变高、菜单栏的快捷键只能在打包版真实窗口里试。
- **体检**：`?mock#/settings/health`（编的几条，点了打开记录）。Rust：`health`。

## 模块

- **导入账单**（Importer）：`?mock#/finance/import`，「导入」拿到编的支付宝账单（4 行，每次另一份），`&finance=images` 两张截图，`&finance=dbs` DBS 的 PDF（带一笔同额的已有记录，测候选）。模拟模型：查重有候选的标「可能重复」，「再整理」把生效的第一条要点写进备注。关掉「钱来钱往 · 收支」的发给云端后「导入」「再整理」不能点。Rust：`finance`、`importer`。
- **一网打尽**：`?mock#/bookmarks`（编的文件夹，没有图标只显示首字母），拖动照 `requirements.md`「一网打尽」一样样试，文件夹拖到自己里面不动。Rust：`bookmarks`、`icons`。
- **呦呦漫游**：`?mock` 里是模拟的网站（「作品」三篇，图片是按网址画的色块）；commit 文本写「失败测试」「上线失败」得到检查没过 / 上线失败。开发版先在页面上点「建副本」（副本和假远端见 `architecture.md`「呦呦漫游」）。在临时副本上跑：`real_site_sandbox` 整套发布（真的 `npm run qa`、提交、推到假远端）；`real_site_edit` 把每个作品和其余每一页读了又存（一个文件都不能变），再改遍各页、跑 `npm run qa`；`real_site_server` 启停真的本地服务。只读真文件夹：`real_site_read`（改动清单和 gh 读的「最近发布」）、`real_yaml_like_js_yaml`（全部内容文件和 js-yaml 逐字比对）。
- **系统通知**：`?mock#/settings/general` 的「通知」：开关、每天几点（填错了退回原来的）。Rust：`notify`。真的通知：临时数据目录的打包版带 `POMELO_REAL_NOTIFY=1` 直接跑程序（看得到 `[notify]` 的日志），库里放一件今天到期的待办、`notify.at` 写早一点，开着 15 秒内弹；第一次会弹系统的允许框，要我本人点（和正式版共用），先告诉我。点通知打开那条要看真窗口。
- **有板有眼**：`?mock#/canvas` 是模拟的服务（「启动」半秒后跑起来；Pomelo Explore 第一页的画板带批准状态；改名、删除、撤销都照做）；右边的 frame 指向 `127.0.0.1:4321`，真开着 visual-atelier 的 `dw serve` 就看得到真的界面（批准状态以它为准），没开是空白。Rust：`canvas`；`real_canvas_bench` 用真的服务（只读真文件夹，自己挑空端口，启动、列画布、停掉）。frame 在打包版里能不能显示要在真实窗口里看（临时数据目录用 4331 端口）。
- **Atlas**：`?mock#/atlas`：三份编的（2026 年写到今天的一份、2025 年两份，各有什么见 `scripts/atlas-mock.mjs` 开头），另有九个没写过的年份好左右滑；数据是 visual-atelier 的样例，`node scripts/atlas-mock.mjs` 重新生成 `src/dev/atlasMock.json`。小报在 frame 里，脚本要进 frame 找（Playwright 用 `frameLocator('.atlas-journey iframe')`）；「立即运行」「补做」三秒后多一份。2026 那份的 `writing`、`writing-moments` 两幕点书签打开那篇文章（见字如面停在设置子页时也是）。Rust：`atlas`、`made`。
  - 真实窗口里直接翻开某一份：临时目录的库里除了 `dev.open` 写 `"/atlas"`，再写 `dev.atlas`（那一份的编号）；`dev.atlasView` 写 `"designs"` 是「样式」那一页，`"2025"` 是那一年做过的每一份；`dev.atlasRun` 写 `true` 立即运行写到今天，`"2025"` 补做那一年（不问、直接写）。都只管这一次启动，用过就自己清掉。要看：小报的字清不清楚、翻页顺不顺。
  - **真模型**（先问我）：年度总结的 Key 只在钥匙串，所以用打包版跑。`sqlite3 -readonly "<正式数据目录>/pomelo.db" ".backup <临时目录>/pomelo.db"`，`youzi.db` 同样拷一份（只拷这两个）；在拷贝里关掉别的定时任务（`UPDATE schedules SET enabled = 0 WHERE builtin IS NULL OR builtin <> 'yearly'`）和简报（偏好 `brief.enabled` 写 `false`），把年度总结的模型和强度改成要测的（`schedules` 的 `model`、`effort`），设置里写 `dev.open`、`dev.atlasRun`；再直接跑打包出来的程序（不用 `open`，才拿得到进程号、看得到日志）：`POMELO_DATA_DIR=<临时目录> POMELO_REAL_MODELS=1 POMELO_NO_INDEX=1 POMELO_ATLAS_DUMP=<文件夹> "<包>/Contents/MacOS/pomelo"`（`POMELO_NO_INDEX=1` 不建索引、不扫知识库，不为这份拷贝启动我的向量服务；`POMELO_ATLAS_DUMP` 存下各幕每一次的请求和回复）。写好了 `year_atlas` 多一行，用 `dev.atlas` 翻开看。新打的包第一次读钥匙串可能弹授权框，要我本人点，没点会一直等（`sample <进程号>` 里停在 `SecItemCopyMatching`），先告诉我再跑。几个模型能同时各开一个拷贝。测完杀掉进程、删掉拷贝和落盘的文件夹（里面是真实记录）。
- **Seeker**：`?mock`：编的资料（林小柚，中英文，七个子页都有内容）、`knowledge/`、`cv/`、`material/`（选了 Seeker 文件夹后才有）；选文件夹、读模板、导出、从 transfer/ 导入都是模拟的（不写文件）；先在「设置」选模型（选云端、没允许发给云端时是失败的样子），模拟调用等一秒半，本次要求写「失败测试」得到失败。Rust：`seeker`（照 `transfer/` 格式编的假文件夹）。
  - 内置简历模板：`POMELO_RESUME_PREVIEW=<文件夹>` 写出两份样例，用 Word 打开看（`qlmanage` 的预览不准）；改中文样式改 `scripts/resume-templates.ts` 重新生成（只读两份模板），生成后查一遍没有个人文字。
  - 看真实规则发给模型的样子：复制一份 youzi.db，`POMELO_PROMPTS_DB=<副本> POMELO_PROMPTS_OUT=<文件.md>` 跑 `prompts_of_a_copy -- --ignored`。
  - 真模型（先问我）：`real_seeker`（`POMELO_SEEKER_OUT=<文件>`）。
- **Rater**：`?mock`：五道编的题和四条规则，先在「设置」选一个本地模型；打的字或反馈里写「失败测试」得到失败；「复盘」每说一句模型把它加进表，说「就这些」标成填好；投石问路腾讯的岗位面板能跳到 Rater。Rust：`rater`。
  - 真模型（先问我）：`real_qbank`（`POMELO_QBANK_CASE=简历题|通用题|突击题|润色`、`POMELO_QBANK_OUT=<文件>`，报告放「下载/Rater 测试/第 N 轮/」）、`real_interview`（`POMELO_INTERVIEW_OUT`）、`real_practice`（`POMELO_PRACTICE_CASE=挑卡|追问|反馈`、`POMELO_PRACTICE_OUT`）。复盘 · 记录走柚子的对话，真模型要在打包版里点（临时数据目录，先问我）。
- **DevJanitor**：Rust：`janitor`（`cleaning_litter`：.DS_Store、Office 的临时文件只清个人文件夹里的、Office 开着不清；`clones_count_once`：在临时目录里用 `cp -c` 做真的 APFS 克隆，共用的只算一次、一项只算它独占的、克隆不算重复，不在 APFS 上就跳过）。
  - `?mock#/janitor/clean`：左边分类（一键清理六类、只列出来七类），选了几项写在贴纸角上；「零碎的小文件」里 .DS_Store 5 个、Office 的临时文件 3 个，各一个按钮。`?mock#/janitor/env`：九类，宽窗口两栏。`?mock#/janitor/backup`：记录有三条，悬停出删除。
  - `?mock#/janitor/disk`：数据是 Rust 扫假 Mac 的结果（`src/dev/janitorMock.json`；改了扫描或假 Mac 用 `POMELO_JANITOR_MOCK=<文件> cargo test janitor_mock_json -- --ignored` 重新生成），「重新扫描」跑两秒。
  - `?mock&youzi=ready#/janitor/agent`：问题里有「删」「废纸篓」→ 移到废纸篓的卡片（含一项受保护的）；「命令」「conda」→ conda clean；「sudo」「JDK」→ 要管理员密码的命令；「规则」「一键清理」→ 加规则的卡片；「看看」「文件夹」多看一次下载，「只读」「du」多一行「跑了 du」，「清单」多一行看清单；执行的命令一秒半后「做完了」。
  - 看真磁盘上的速度和准确性先问我：`cargo test --release --manifest-path src-tauri/Cargo.toml real_janitor_scan -- --ignored --nocapture`（只读，跳过 Pomelo 的数据目录、Seeker 文件夹和没权限的地方；`POMELO_JANITOR_SHOW=1` 列出开发目录、App 残留和开发环境，不列个人文件名）。真删、真跑命令、完全磁盘访问权限只在正式版里由我验收。
  - **备份**：Rust：`backup`（假 Mac 的「下载」对假移动硬盘 `janitor-volumes/小柚的硬盘`：对比的分类、复制带时间和标签、被覆盖的进 ARCHIVE、停下不留半个文件、跟着改名、VAULT 两边都不出现、Agent 也看不到 VAULT；清单：两边最上一层的文件夹（ARCHIVE 不列）、勾上的「下载」文件夹变成同名的任务（编号留着）、新的默认不勾、旧设置（任务和不备份的文件夹）插上硬盘时变成清单；柚子的线索认出勾上的 `小柚` 在硬盘的 `回忆/小柚`（提议改成同名）、`照片/其它` 在电脑上改叫 `其他`、没勾的只说一句，只在硬盘上的 `影音`、`回忆_2606` 不提；改名卡片提议时、点了时的检查和结果（不挪进、挪出没勾的和 ARCHIVE；以前的去重、任务卡片点了只说没有了），模拟模型照线索出的卡片都过得了检查；没勾的文件夹：线索、卡片都不碰）。`?mock#/janitor/backup`：顶上切「备份 · 清单」；两栏，Downloads 勾了三个、都有要复制的，PikPak 列出硬盘上勾了的四个（三个有事做）；清单勾了五个（`回忆_2606`、`作品` 没勾），勾 `作品` 多一个要复制的、取消就少一个；柚子已经看过一眼（一张改名卡片：改名把挪了的拿掉、`小柚` 照硬盘上的算），「柚子看一眼」一秒半，「开始备份」跑三秒、完了弹便签、PikPak 再对比一秒，「在硬盘上跟着改」把改名的拿掉，「重新对比」一秒；`&backup=none` 还没选硬盘。`?mock&youzi=ready#/janitor/agent`：问题里有「备份」「硬盘」→ 同一张卡片，「查了 7 处」一行太长时截断。
  - **PikPak**：Rust：`backup` 里的 `the_cloud_`、`pikpak_`、`rclone_`（后两种要装 rclone，没装就跳过并打一行字）：对比照同步的规则算（一样、大小不同、挪、只在 PikPak 上，只算要的文件夹）；rclone 把假硬盘勾了的文件夹同步进 `janitor-cloud/PikPak`（一个本地文件夹当 PikPak）：同步前对出来全是新的、和硬盘一样、没有 VAULT 和系统小文件、名字是 NFC、对比后再同步什么都不传、硬盘上改了名的文件夹在云上是挪、大小变了的重传且旧的进 ARCHIVE、只在云上的进 ARCHIVE、云上的 ARCHIVE 和没勾的不碰、比对出来的多 100 个就停、限速 1 MB/s 时中途停下云上没有半个文件、下次接着传；名字里的 : ? * 换成 _ 后和云上 PikPak App 传的对上、换完撞名的写出来、「名字(1)」的错误认成卡住的名字、卡住的文件夹整个挪进 ARCHIVE 后看得见的挪回来（用一个 .DS_Store 当看不见的半成品）；登录时配置是加密的、退出就删掉；记录带同步的文件夹，按硬盘和 PikPak 分开收尾。`?mock#/janitor/backup`：PikPak 已登录、对比过，「开始同步」跑五秒、完了弹便签、那几个文件夹变成一样，能停下；`&pikpak=none` 没登录（密码写 `wrong` 登不上）。真 PikPak 只在正式版里由我登录、验收。
  - **柚子看一眼的真模型**（先问我）：`POMELO_CLOUD_KEY_FILE=… [POMELO_CLOUD_SERVICE=… POMELO_CLOUD_MODEL=…] [POMELO_THINK=1] cargo test real_backup_look -- --ignored --nocapture`：临时目录里的假 Mac 和假硬盘（清单勾了除 `作品` 以外的），用自动看一眼的问题跑真的 Agent 循环和工具，打出它查了什么、回答、每张卡片（照 App 的检查建），最后数该出的两张改名卡片出了几张、有没有说到没勾的 `作品`、有没有多嘴说归档的 `影音`；不读任何真数据。
  - 备份的真机只读检查（不写任何东西，只打印数量；碰真硬盘先问我）：`real_backup_volumes`（认出插着的移动硬盘）；`POMELO_BACKUP_PAIRS="<电脑上的文件夹>=><硬盘上的文件夹>;…" cargo test real_backup_compare -- --ignored --nocapture`（对比每一对，看分类和用时，断言没有 VAULT）。真复制只在正式版里由我点「开始备份」验收。
- **Pub2MD**：`?mock`：「选择文件…」依次拿到一篇论文、「失败测试」（第 3 块出错一次，能「继续」或「删除」）、两张照片、「读书笔记.md」（只翻译），选了先列在「待发送」，点「发送」才开始。真测见「真模型和真接口」。

## 柚子

- **问我（ASK）**：`?mock&youzi=ready#/youzi`，先 `window.__TAURI_INTERNALS__.invoke('youzi_cloud_key_save', { service: 'deepSeek', key: 'x' })` 和 `youzi_select` 选云端模型（本地模型不问），问题里写「问我测试」出三题的卡片，「问我中断测试」模拟等待时退出。Rust：`ask`。真模型（先问我）：`POMELO_CLOUD_KEY_FILE=… POMELO_ASK_OUT=<文件>` 跑 `real_ask`，报告放 `~/Downloads/ASK 测试/`。
- **思考强度**：`?mock&youzi=ready#/youzi` 用 `window.__TAURI_INTERNALS__.invoke('youzi_select', { choice: 'cloud:haiku-5-5' })` 换模型看思考按钮：Haiku 5.5 五个强度、默认 xhigh；DeepSeek Flash 三个、默认 high；Qwen3.8 Flash 没有强度；GLM 只能思考；本地模型是开关（设置 → 云端模型能改）。跟着模型变的还有：定时（任务和今日简报）、Translator、`#/rater/settings`、`#/seeker/settings`、Pub2MD、导入账单、设置 → 柚子的「面板帮手」。Rust：`cloud`、`provider`（`level_for`）、`library`、`schedule`、`brief`、`yearly`、`seeker`。
- **面板帮手**：`?mock&youzi=ready`。「一句话填好」的模拟模型把「：」后面当内容 / 备注、http 开头的词当链接、「每天」当每天的重复（有「打卡」是打卡）、「退款」勾退款；「一句话改好」只写它找到的，「不要链接」清空链接。念念不忘卡片「⋯ → 柚子转为待办」每行一张卡片（规则同上，「下周X」是截止日，第一张放第一个分组）。Rust：`assist`、`tasks_made_from_notes`。
- **Excel 工具箱**：`?mock&youzi=ready#/youzi`，问「存成 Excel」得到新建的卡片，「改预算表」得到改格子和加删行的卡片。Rust：`excel`（照 Excel 的结构编的 .xlsx，写完用 calamine 读回来比）。`POMELO_EXCEL_PREVIEW=<文件夹>` 跑 `excel_samples -- --ignored` 写出样例，`qlmanage -t` 能预览；用真的 Excel 打开（不弹修复提示、公式算得出来）由我来，或者我同意电脑操作时由 Claude 来。
- **定时任务能联网、用工具箱**：`?mock&youzi=ready`，建一个提示词写「联网查最新的新闻，存成 Excel」的任务，立即运行，打开它写的对话看卡片。Rust：`skills`。
- **看文档里的图片**：`?mock` 问「文档里的图」看「查了」。Rust：`office`、`says_which_pages_hold_images`。
- **年度总结的「留对话」**：`?mock&youzi=ready#/youzi` →「定时」→ 年度总结，「留对话」关着、开着各立即运行一次，看对话留不留、开着时「上次」能点（该怎么样见 `requirements.md`「定时任务、今日简报、年度总结」）。Rust：`youzi::schedule`。
- **流程图**（插件库、定时里的年度总结）：`every_flow_joins_its_own_steps`（每张图首尾是一步，分支接回、循环回到的都是这张图里的步骤）；改了 Rust 里的流程，跑 `skill_flows_json -- --ignored --nocapture` 拿 JSON，替换 `mockIpc.ts` 里 `P_SKILLS` 的 `flow`（年度总结是 `FLOW yearly` 那行，换 `pYearlyFlow`）。
- **搜索速度**：`cargo test --release --manifest-path src-tauri/Cargo.toml search_speed -- --ignored --nocapture`（20 万条编的数据：按字 0.1 秒内、按意思 0.3 秒内）。

## 真模型和真接口（都先问我）

各测试的开关写在它开头的注释里，下面只写常用的。

- **云端**（一律用它）：`POMELO_CLOUD_KEY_FILE=<放 Key 的文件>`（`~/Downloads/ELEN/Pomelo/key/` 里每家一个），下面写 `POMELO_REAL_LLM=1` 的都换成它；默认 DeepSeek Flash；`POMELO_CLOUD_SERVICE`（deepseek、openrouter、openai、anthropic、google、aliyun）和 `POMELO_CLOUD_MODEL=<模型>` 换服务和模型。思考关不掉的模型（OpenRouter 的 GLM 5.3）加 `POMELO_CLOUD_THINK=always`，强度用 `POMELO_CLOUD_EFFORTS=low,high,max`。测试打出云端的原始报错（`[cloud]` 开头），App 里不显示。只用编的数据。
- **本地模型**（只在我特别要求时）：`~/Documents/LLM/scripts/llmctl.sh start qwen359b`，等 `curl -sf 127.0.0.1:8080/health` 通了再跑，用完马上 `llmctl.sh stop`，测试里写 `POMELO_REAL_LLM=1`。一次跑一个问题、给个时限：Qwen3.5 9B 偶尔会想很久停不下来。
- **柚子**：`POMELO_REAL_LLM=1 POMELO_ASK='问题1|问题2>>追问' cargo test --manifest-path src-tauri/Cargo.toml real_local_model -- --ignored --nocapture`（导入文件加编的数据，在内存里；`POMELO_SHOW=1` 显示查到的原文，`POMELO_AS_CLOUD=1` 按云端的规则，`POMELO_SCHEDULED=1` 按定时任务只读）。
  - 润色补全、一句话填好：`real_assist`、`real_quick`（一句话改好、柚子转为待办只用模拟模型，在 `assist` 里）。
  - 今日简报：`real_brief`（`POMELO_TODAY` 换日期）。
  - Translator：`real_translate`（`POMELO_TR_CASE=zh|keywords|long`，一次一个；`POMELO_THINK=0` 不思考，开思考时 9B 两句话也会想几分钟）。
  - 长资料补读：`real_read_file`（编的 60 章手册，`POMELO_ASK` 换问题）。
  - Word / 文本 / Excel 工具箱：`real_toolbox`（编的文件，打出调用、卡片和写入后读回的结果；`POMELO_TB_CASE=kb,table,excel-cells…` 挑几个，`POMELO_TB_DUMP=1` 只看读出来的段落）。
  - 定时任务：`real_schedule`（定时任务的工具，搜索结果是模拟的；`POMELO_ASK` 换提示词）。
  - 看图：`real_file_images`（编的带图的 Word、PowerPoint 和扫描 PDF；`POMELO_IMG_CASE=docx,pptx,pdf` 挑，`POMELO_IMG_OUT=<文件夹>` 只写出这几份文件和编的预算表，拿去 App 里试）、`real_kb_image`（知识库里的图）；只能用云端。
  - 呦呦漫游的补全、按中文起草：`POMELO_REAL_LLM=1` 跑 `real_site_draft`（编的作品）。
- **向量和重排**：`llmctl.sh start embedding`（或 `rerank`）后跑 `real_embedding`、`real_rerank`，用完停；`real_services_start_and_stop`、`real_meaning_search`、`real_knowledge`（临时目录里编的文件夹）自己启停（先确认都没在跑）。
- **导入账单**：样本 `~/Downloads/ELEN/Pomelo/pomelo-finance-data`（只读），答案在 `phase5-r2-evaluation/gold.json`：`POMELO_CLOUD_KEY_FILE=… POMELO_CLOUD_SERVICE=… POMELO_CLOUD_MODEL=… POMELO_IMPORTER_DIR=<样本> POMELO_IMPORTER_OUT=<结果.json>` 跑 `real_importer`（强度 `POMELO_IMPORTER_LEVEL=max`）。只在内存里，只写结果文件；报告放 `~/Downloads/Importer 测试/第 N 轮/`。
- **Pub2MD**（先说用量，我同意再测；Mathpix 按页收费）：Key 文件我给；结果放 `~/Downloads/Pub2MD 测试/第 N 轮/`（过程放 `运行记录/`，写进 `测试记录.md`）。`cargo test --manifest-path src-tauri/Cargo.toml real_pub2md -- --ignored --exact youzi::assistants::pub2md::job::tests::real_pub2md --nocapture`：真的 Mathpix 和云端模型走完整个流程，打出块数、请求、用时和每个模型的词元用量。跑的时候用 Monitor 边算费用边盯输出（到我给的上限就停）。`real_pub2md_probe` 单问 Supervisor 一步（`POMELO_PROBE=terms|fix|plan|read|search`，不用 Mathpix）。改了切块或检查，`POMELO_REPLAY=<以前的结果文件夹>` 跑 `replay_` 拿真结果回放。
- **书单 / 影单检索**：`POMELO_REAL_WEB=1` 跑 `real_works`（示例书名，可以直接跑）；柚子的真模型测试加 `POMELO_REAL_WEB=1` 就用真检索。Tavily 要我的 Key，真的联网搜索只在正式版里试。
- **网站图标**：`real_site_icons`（几个公开网站，只取首页和图标）；正式版打开一网打尽时才去取。
- **真实导入文件**（`prototype/` 里的 `pomelo-import.json` 等，只在本机）：`POMELO_SEED_DB=<临时目录>/pomelo.db` 跑 `seed_scratch_db -- --ignored` 生成含全部导入文件的临时数据库；`transfer::tests::real_ -- --ignored` 跑读它们的测试。

## 界面原型

`prototype/ui-rN/` 改 `app.js` / `style.css` 后跑 `node build.mjs` 合成单个 `index.html`（双击能开）。预览面板直接打开 `file://` 地址，打开的是快照：改完要重新打开，刷新不算。
