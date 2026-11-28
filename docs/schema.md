# 数据结构

`pomelo.db` 50 张表、版本 43；柚子的 `youzi.db` 56 张表、版本 45；搜索索引 `search.db` 版本 3；知识库扫描件的识别结果 `ocr.db` 版本 1；DevJanitor 的 `janitor.db` 版本 2；数据文件格式 43。这里只写有哪些表、存什么、怎么关联，需求里没写的计算规则、数据文件格式和版本记录；功能和看得到的规则见 `requirements.md`，存放、备份、撤销、加密的做法见 `architecture.md`，细则在各 Rust 文件开头（模块的在 `modules/<模块>.rs`）。

## 通用约定

- 多数表有 `id`、`created_at`、`updated_at`，常带 `note` 备注、`sort_order` 手动排序（小的在前，新加的排最后），下面不写；`xxx_id` 指向那张表。
- 日期 `2026-09-28`、年月 `2026-09`、年 `2026`；选项存英文代码。文字去首尾空格，空的存成「没填」。
- 删上一级时下面的一起删，另写明的除外。改数据都能 ⌘Z，不进撤销的另写明（见 `architecture.md`「撤销」）。

## 状态、类型、设置

书单、影单、游戏、绘画课节共用一套状态：

| 代码 | 摸鱼读读 | 好奇看看 | 玩物有志 | 随意画画 |
|---|---|---|---|---|
| `unmarked` | 待标记 | 待标记 | — | — |
| `todo` | 想读 | 想看 | 想玩 | 未开始 |
| `doing` | 在读 | 在看 | 在玩 | 进行中 |
| `done` | 已读 | 已看 | 通关 | 已完成 |

- 新建的书影是 `unmarked`，游戏、课节是 `todo`；循序渐进也没有 `unmarked`。
- **完成时间**：书影游记年，课节记年月；标完成时没填就填今年 / 当月，改回别的状态不清（课节圆圈上的「取消完成」例外，等于撤销）。

**kinds**（能自己改的类型）：`module`、`name`（同模块不重名，最多 20 字）。记录只存 `kind_id`，删了变「未分类」。各组（默认值写在 migration 里）：`books`、`films`、`games`、`game_platform` 平台（列是 `games.platform_id`，删了变没填）、`drawing`、`packing`、`fin_out` / `fin_in` 支出 / 收入分类、`job_channel` 投递渠道；每个循序渐进模块一组 `progress/编号`（开始是空的）。

**settings**（`key`、`value` JSON；不进撤销和导出）：

- 界面：`ui.dock` 贴纸簿的分类、顺序和置顶 `{ cats: [{ id, name, tone 分类色, mods }], pinned }`；`ui.module_hidden` 隐藏的模块（搜索、柚子也照它）；`ui.module_look` 挑的贴纸和颜色 `{ [模块]: { mark?, tint?: mint|pink|grape|orange|sky } }`（没写的用自己的，模块删了一起删）；`ui.desk` 首页便签位置 `{ notes: { [便签]: [x, y] } }`（没写的在默认位置）；`ui.tabs` 开着的页（各页的路径）；`ui.site_tab`、`bookmarks.at`、`canvas.*` 这类记上次停在哪。
- `appearance` `{ palette: blue|purple|kraft|yellow|mint, theme: system|light|dark }`（默认蓝笔、跟随系统）；开机启动这类开关；`site.folder` 网站文件夹（空 = 默认）。
- 通知：`notify.kinds` `{ todo, jobs, days, schedule }` 开关（没写算开）；`notify.times` 前三种每天几点 `HH:MM`（只写改过的）；`notify.at` 其余的默认（没有就 09:00）；`notify.sent` 每种上次哪天看过。
- 速记小窗：`capture.draft` `{ tab, note, todo, due, group, ask }`（`due` 空 = 今天）、`capture.files` 草稿里的附件编号（清理附件时算在用）、`capture.pin` 图钉。

## 附件

- **attachment_files**：`name` 原文件名、`stored` `attachments/` 里的随机名（空 = 不在这台 Mac 上，比如从数据文件导入的）、`kind`（`image` / `file`）、`bytes`、`sha256`。
- **attachments**：`owner`（`note` / `task` / `goal` / `article`）、`record_id`、`file_id`。不设外键：清理掉的文件被撤销带回来时显示「不在这台 Mac 上」。见字如面的按名字写进正文，只挂正文里用到的（`articles.rs` 开头）。

## 摸鱼读读 · 好奇看看 · 玩物有志

- **books** / **films**：`title`（必填）、`author` / `director`（多人用「 / 」隔开）、`country`、`kind_id`、`status`、`rating`（1～5，可空）、`finished_year`。**book_rereads** / **film_rereads**（第二次起每一次）：`book_id` / `film_id`、`year`、`rating`。
- **games**：同 `books`，没有作者、多了 `platform_id`；`finished_year` 是一周目。**game_replays**（二周目起）：`game_id`、`year`、`rating`。周目 = 一周目（通关、填了年份或有后面的周目才算）+ 后面的个数。「搬到玩物有志」（`from_list`）的对法见 `requirements.md`「游戏「玩物有志」」。

## 随意画画 · 出门走走

**drawing_courses**：`name`（不重名）。**drawing_lessons**：`course_id`、`code` 编号（如 `2B`，只显示）、`kind_id`、`title`（必填）、`status`、`finished_month`。

**packing_templates**：`name`（不重名）。**packing_template_items**：`template_id`、`kind_id`、`name`。**trips**：`name`、`template_id`（只作记录，模板删了出行还在）、`depart_on`。**trip_items**：`trip_id`、`kind_id`、`name`、`packed`；建出行时复制，之后各改各的。

## 一步一步

**todo_groups**：`name`（不重名）、`icon`（图标库的 `i:<键>`，可空）。**todo_goals**：`group_id`、`name`、`icon`、`status`（`active` / `paused` / `done`）、`start_date` · `end_date`、`review` 复盘。

**todo_tasks**：

| 字段 | 含义 |
|---|---|
| `group_id` · `goal_id` · `parent_id` | 目标可空；`parent_id` 空 = 顶层，子任务跟着顶层的分组和目标 |
| `title` · `note` · `link` | `note` 能 `**加粗**` |
| `from_note` | 转自哪条笔记（笔记删了变空） |
| `task_type` | `normal` / `repeat` 重复 / `checkin` 打卡 / `progress` 进度条 |
| `start_date` · `due_date` | 时间段（开始可空）；重复任务的 `due_date` 是「这一次」，按规则推算 |
| `plan_date` | 重复（完成后）从哪天开始 |
| `repeat_rule` · `repeat_total` · `done_before` | 重复 / 打卡的规则、共几次（空 = 不限）、用 Pomelo 前做过几次 |
| `target` · `unit` | 进度条的目标数量、单位 |
| `done` · `done_at` · `collapsed` | 完成、完成时间、子任务折叠 |
| `repeat_until` · `period` | 旧字段，不用（前者总是空，进度条的后者总是 `all`） |

**todo_logs**：`task_id`、`day`、`amount`（默认 1）；重复每做完一次、打卡每天、进度条每记一笔各一条。

**重复规则**：`daily`、`weekly:1,2,3,4,5`（周一 = 1）、`monthly:1,15`（没有这天用月底）、`yearly:12-31`（`/3` 每 3 年；平年 02-29 用 02-28）、`every:3d` / `every:2w`、`after:7d` · `2w` · `6m` · `1y`（完成后过多久）。打卡只能每天或每周几天。

- **重复**：从今天和开始日期里晚的那天起找第一次；做完一次记一条，挪到这一次和做完那天之后的下一次；次数做满就整个完成；「跳过这次」只挪日期。做过几次 = `done_before` + 记录数。
- **打卡**：没开始日期的从添加那天算；取消一天变回没完成；补打只在时间段里、今天以前。

## 钱来钱往

金额存「分」的整数（持仓单价除外）。币种 `CNY` / `SGD` / `HKD` / `USD` 从不相加，只有总资产按汇率折成人民币。

| 表 | 字段 |
|---|---|
| **fin_accounts** 账户 | `name`（不重名）、`kind`（`bank` / `wallet` / `broker`）、`currency`（记过余额或持仓后不能改）、`closed` 停用 |
| **fin_balances** 余额 | `account_id`、`day`、`amount`（每账户每天一条；券商的就是总值） |
| **fin_rates** 汇率 | `day`、`currency`、`rate`（1 单位合多少人民币） |
| **fin_holdings** 持仓 | `account_id`（只能券商）、`day`、`name`、`quantity`（> 0）、`price` 单价（元）；保存时替换那天的全部持仓 |
| **fin_entries** 收支 | `flow`（`out` / `in`）、`amount`（> 0）、`currency`、`day` · `end_day` 时间段（一天的 `end_day` 空）、`kind_id`、`fixed_id` 哪个固定支出记的（删了留着）、`is_refund` 退款（只能是一天的支出）、`refund_of` 退的哪笔（删了变空） |
| **fin_imports** 账单 | `name` 文件名（「选原图」靠它认）、`title`（空 = 文件名）、`sort_order`（新的最小）、`source` 模型认的来源（`alipay` / `wechat` / `dbs` / `wise` / `icbc`，别的 `screenshot`）；不存原件 |
| **fin_import_rows** 账单的一行 | `batch_id`；`source_json` 读到的原文（各列、疑点、模型的第一稿），不再改；`draft_json` 核对稿（日期、金额、币种、算作 `out` / `in` / `refund` / `neutral`、分类名、备注、退的哪笔）；`state`（`pending` 待核对 / `ready` 核对过 / `posted` 入账 / `neutral` 不计收支 / `duplicate` / `ignored`）；`entry_id` 入账成哪笔（删了变空、状态留着，再导入不会又冒出来）；`resolution`（`new` 不是重复 / `linked` 关联到已有的）；`ai_json` Importer 做过什么（模型、疑点和查重的结论与理由、批量改的分类、出错原因） |
| **fin_fixed** 固定支出 | `name`（不重名）、`flow`、`amount`、`currency`、`kind_id`、`rule`（`monthly:15` / `yearly:03-01`）、`next_day` 下次哪天记、`stopped` |
| **fin_budgets** 预算 | `kind_id` 支出分类、`currency`、`amount` 每月（每分类每币种一条） |

- **总资产**：那天缺某币种的汇率就不算总数，并提示缺什么。
- **时间段**（旧的小额记录，不能新记）按天平摊，跨月按天数分到各月（四舍五入到分，加起来正好等于金额）。
- **退款**：合计、报表、预算、首页、柚子都从支出里减；关联了原支出的分类跟着原支出，退的总数不超过原支出。
- **账单入账**：不计收支的只留在账单里；关联到已有记录的不再记，几个来源共用一笔；固定支出记的那笔，备注写它的名称。

## 投石问路

**job_statuses**：`name`（不重名）、`stage`（`wish` 准备 / `applied` 等回音 / `active` 进行中 / `got` 拿到 / `closed` 结束；定规则，界面不显示也不改，新加的是 `active`）。有投递正处在或时间线上有它的不能删；时间线上有它的不能改成准备（`update_status`）；至少留一个。

**jobs**：`company`（必填）、`position`、`location`、`link` JD、`kind_id` 渠道、`resume` 旧的简历位置（不显示；`cv/` 里的已搬成 Seeker 的岗位简历）、`followed_on` 最近一次「已跟进」。

**job_steps**（时间线）：`job_id`、`status_id`（不能是准备阶段，同一投递不重复）、`done`、`done_on`（可空）；按状态列表的顺序。当前状态、投递日期、该跟进都不存，每次算（`jobs.rs` 开头）。

## 念念不忘

- **notes**：`content`、`pinned`、`created_at`（精确到毫秒、不重复，数据文件里靠它认）。改置顶不更新修改时间；哪天记的按本机时间；置顶的只在没筛选时排最前。
- **note_tags**：`note_id`、`tag` 完整路径（如 `读书/摘抄`），每次保存从正文重新提取。
- **note_refs** 引用：`note_id` 引用的、`ref_id` 被引用的、`sort_order`；任一边删了就没了。
- **note_tag_meta** 标签设置：`tag`、`pin_order` 置顶先后（只有第一级）、`icon`（`i:<键>` 是图标库的，别的是 emoji）、`sort_order` 同一层里拖出的位置（空 = 没拖过，排在拖过的后面、按拼音）。改名（拖到别的标签下也算）时跟着搬，合并、删标签时怎么改设置和正文见 `tags.rs`；标签和设置都空了删掉那行。

## 见字如面

标签规则和念念不忘共用 `modules/tags.rs`，表各管各的；列表、排序、废纸篓、加密的规则在 `articles.rs` 开头。

- **articles**：`content`（Markdown）、`title` 从正文算（第一行有字的，去掉记号）、`sealed` 密文（加密的那篇 `content` 空，标题和标签不加密）、`pinned`、`archived`、`trashed_at`（空 = 没进废纸篓）、`created_at`（精确到毫秒、不重复）。
- **article_tags**：`article_id`、`tag`；每次保存从正文重新提取（代码里的 `#` 不算），加密的留着加密前的。**article_tag_meta** 同 `note_tag_meta`。

## 柚子链接（跨模块联动）

不另存表：链接就是念念不忘和见字如面正文里的字（见字如面代码里的不算），谁链到这里每次从正文算。写法和目标见 `requirements.md`「跨模块联动「柚子链接」」，细则（同一分钟的笔记写到秒、毫秒，重名认先加的，Bear 的转义，柚子链接一栏的排法，导出时嵌进来的笔记变引用 `quote_embeds`）在 `modules/links.rs`。

## 来日方长

- **days**：`kind`（`day` 纪念日和倒数日 / `birthday` / `document` 证件）、`name`、`date`（不知道年份的生日写 `--10-17`）、`repeat`（`none` / `year` / `month` / `week`；生日总是每年，证件总是不重复）、`remind` 提前几天（`30,7,0`，大的在前，最多 5 个，0 = 当天）、`pinned`（最多一条）、`ideas` 礼物点子（只有生日）。
- **day_gifts** 送过的（只有生日）：`day_id`、`year`、`gift`。

## 一网打尽

**bookmarks**（文件夹和网址同一张表）：`parent_id`（空 = 最外层，只放文件夹）、`kind`（`folder` / `link`）、`title`、`url`（只收 http / https；文件夹空）、`on_home` 放首页（只有网址）、`sort_order`（同一文件夹里文件夹和网址混排）。网站图标不在库里，是数据目录 `icons/` 的缓存（`icons.rs`）。

## 五花八门 · 循序渐进

- **custom_modules**：`template`（`list` / `progress`）、`name`（不重复，最多 20 字）、`icon`（`ICONS` 24 个之一；挑了没对应图标名的贴纸时不变，贴纸和颜色在 `ui.module_look`）、`course_word` · `lesson_word` 两层的叫法（只循序渐进）。编号永远不重用，模块键 `list/编号`、`progress/编号`。
- **list_fields**（一个清单最多 20 个）：`module_id`、`name`（清单内不重复，不能叫「名称」）、`type`（`text` / `longtext` / `number` / `year` / `select` / `multi` / `check` / `rating` / `link`）、`options`（JSON，最多 50 个）、`shown` 表格里显示。
- **list_items**：`module_id`、`name`、`data` `{"字段编号": 值}`（空的不存；单选存选项名，多选存选项名列表，勾选只存「是」，评分 1～5，年份存 `"2026"`（1900～2100））。
- **progress_courses**：`module_id`、`name`（同模块不重复）。**progress_lessons**：`course_id`、`code`、`kind_id`（`kinds.module` = `progress/编号`；空 = 未分类）、`title`、`status`（`todo` / `doing` / `done`）、`finished_month`。

## 芝麻开门

数据库里只有密文（加密见 `architecture.md`「芝麻开门的安全」），不进数据文件；备份里是密文，恢复较早的备份后要用那时的主密码。

- **vault_meta**（一行，不进撤销）：Argon2id 参数 `kdf_salt` · `kdf_m` · `kdf_t` · `kdf_p`、`wrapped_key` 锁着的密码库钥匙（改主密码只换锁）、`key_check`、`master_at` 上次输主密码、`touch_build` · `touch_account` Touch ID 那份由哪个版本存、在钥匙串哪项、`touch_locked` 要先输主密码。
- **vault_items**：`data`（名称、链接、用户名、账号、密码、备注合起来加密）、`sort_order`（全 0 = 没拖过，按名称排）、`category_id`（分类删了变空）、`archived`（不改「修改于」）。
- **vault_categories**：`data`（分类名，加密，不重名，最多 20 字）。

## 首页、月历、体检

没有表，每次打开现算（隐藏的模块不算），规则在 `home.rs`、`calendar.rs`（一次最多 62 天）、`health.rs` 开头。

## 柚子（youzi.db）

自己的升级脚本在 `migrations/youzi/`。不进撤销的除了 `architecture.md`「撤销」列的，还有提议和写文件。挂在岗位上的都记 `job_id` + `job_created_at`（按编号 + 创建时间认岗位）；`body` 都是 JSON。

**Seeker**（`body` 的细则在 `seeker.rs`、`seeker/` 各文件开头）：

- **seeker_settings**（一行，不进撤销）：`folder`（只记位置，导入预览时才读）、`options`（模型、思考和强度，各功能单独选的 `features`，允许发云端，页面语言，公司文件夹的自定义名称 `companyNames`）、`template` · `template_en`（不用，导出用内置模板）。
- **seeker_draft**（每种语言一行，`lang` `cn` / `en`）：`body` 草稿（页首、章节 → 经历 → 要点、JD、为哪个岗位；经历记着来自哪条素材的哪一版；`notes` 批注）；`baseline`「修改差异」的底稿。所有块的编号不空、不重复。
- **seeker_records**（资料，一条一小块）：`body.kind`（`material` 素材 / `profile` 基本信息 / `fact` 已确认事实和经历档案 / `rule` 规则 / `example` 豁免情况 / `application` · `application_line` 网申）、`body.category` 分组（必填）、`body.lang`（`cn` / `en` / 空 = 中英共用）、`body.enabled`「用于选材 / 发给模型」。列：`source` 从 `transfer/` 导入的来源（`路径#标题:第几个`，唯一）、`revision`（每次保存加一，要和打开时的一样）、`sort` 自己拖的顺序。**seeker_record_history**：每次改或删之前的样子。
- **seeker_exports**（不改、不进撤销）：每次成功导出的文件名、位置、语言、时间、用的素材编号；不存简历副本。
- **seeker_groups**（键 `kind` 哪一页 + `lang` + `category`）：`label` 自带组显示的名字（空 = 原名）、`sort` 组的先后（空 = 排在有顺序的后面）。
- **seeker_job_resumes**：岗位用过的简历：`path`（Pomelo 导出的或 `cv/` 里别的）、`body`（当时的岗位、导出时的 JD）、`current`「本次使用」（每个岗位最多一份）；文件不在了只是不显示。偏好 `seeker.jobResumesAdopted` 记着 `jobs.resume` 搬过没有。
- **seeker_job_companies**：给岗位手选的公司资料 `folder`（`knowledge/` 下的文件夹名，空 = 不用）；没这一行就按公司名自动对上。

**Rater**（`body` 的细则在 `rater.rs`、`rater/` 各文件开头）：

- **rater_questions**：一题一行，`body`：类别（简历题 / 通用题 / 突击题）、问题、答案、标签、挂的 Seeker 素材或岗位、改自哪道；`revision`、`sort` 同 Seeker 资料；秒数不存，读时算。**rater_question_history** 同 `seeker_record_history`。
- **规则**存在 `seeker_records` 里（用途「题库维护」「卡片练习」「面试」「复盘」和三个题库 SKILL 的工作流）；程序自带的不存。
- **rater_rounds**（卡片练习）：一轮一行，`body`：出卡方式、起止时间、点评和每张卡（哪道题、当时的问题、追问卡和参考答案、会 / 不熟、打的字）。没结束的最多一轮。每题的练习记录不存，读时从各轮算。
- **rater_interviews**：`kind` `prep` 准备 / `review` 复盘 / `summary` 总结，挂在岗位上。`body`：准备（挑的题、可能的追问、速查卡……）、复盘（每个问题、反馈、判断；早先的还带「对照」，不再新做）、总结（对照哪份准备和哪几次复盘：准备到和没准备到的、建议的题、提议的规则要点）。
- **rater_records**（复盘 · 记录；不进数据文件）：没记完的复盘，一条对话一行：`session_id`（删对话时一起删）、`body`（表、模型标的「填好了」、填过几次、开始时间）。它的对话私密、知识库打开，不进对话列表、对话搜索、⌘K 和贴纸簿的对话数；存成复盘时对话和这一行一起删（一步撤销）。

**preferences**（`key`、`value` JSON，不进撤销；API Key 只在钥匙串）：

| 键 | 内容 |
|---|---|
| `model.selection` · `model.last_local` | 选的模型（`local` / `cloud:<id>`）、上次的本地模型 |
| `cloud.models` | 常用云端模型（顺序即菜单顺序）：名称、服务、接口模型名、上下文、回复上限、思考 `never` / `switch` / `always`（没写算 `switch`）、`efforts` 接口用的强度词（空 = 不支持）、`defaultEffort` 固定的默认（可没有）；第一次打开放两个 DeepSeek，删了不再加回 |
| `data.cloud` | 各模块能否发给云端（只存改过的）；钱来钱往分 `finance/flow` 收支、`finance/assets` 资产，旧键 `finance` 算资产 |
| `capture.chat` | 速记小窗在聊的对话（`sessions` 编号；私密，不进列表和搜索，只有一个） |
| `memory.enabled` | 记忆总开关（默认开） |
| `skills` · `skills.contact` | 各技能的「启用」「柚子自己决定用」（只存改过的）；书单 / 影单检索的联系邮箱 |
| `assist.model` · `assist.level` | 面板帮手的模型（`local:<key>` / `cloud:<id>`，空 = 柚子现在选的）、思考（`off` / `on` 模型的默认（默认）/ 某个强度） |
| `brief.enabled` · `.model` · `.thinking` · `.effort` · `.prompt` | 今日简报的开关、模型、思考、强度、提示词（空 = 默认的；`brief.rs` 开头） |
| `assistant.importer` | Importer：上次的模型（`cloud:<id>`）、思考 `level`（同 `assist.level`）、提示词清单 `points`（`{ text, on }`） |
| `assistant.pub2md` | Pub2MD 的设置（步骤、格式、语言、两个模型和 `workerLevel` / `supervisorLevel`（同 `assist.level`）等）和最近 10 份（`pub2md/mod.rs`） |
| `assistant.translate` | Translator 的上一次：设置、原文、译文、怎么结束的、用的模型 |
| `rater.options` | Rater 的模型、思考和强度：默认的和各功能单独选的 `features` |
| `rater.word_folder` · `work.folder` | 速查卡「另存 Word」、「保存为 Word」上次的文件夹（后者不在了用「文稿」） |
| `ui.*` | 上次打开的对话、列表收起、收起的项目、归档展开、开着的助手 |

DevJanitor 的偏好见「DevJanitor 的扫描结果」。

### 对话和项目

**sessions**：`title`、`title_source`（`auto` 取第一个问题的第一行、最多 40 字 / `user`；改成空回到自动）、`active_root_id` 显示的路径从哪条开始、`project_id`、`pinned_at`、`archived_at`、`sort_order`、`instructions` 对话指令、`preset_id`（删预设时清空）、`memory_disabled`、`thinking` · `effort`（空 = 模型的默认）、`knowledge` 知识库开关、`private`、`scheduled` 定时任务写的（任务删了也还是）· `schedule_id` · `unseen` 还没打开过（不进撤销）。`updated_at` = 最后一次发消息，整理操作不碰。删对话连消息和调用记录一起删。

**manual_lists**：拖过顺序的列表（`pinned`、`recent`、`archived`、`project:<id>`）；没拖过的按最后发消息的时间排。

**projects**：`name`（最多 60 字，空的叫「未命名项目」）、`instructions` 背景指令、`include_global_memories`（默认开）、`sort_order`（新的在最上）。删项目不删对话：它的对话移到「最近」最上面，置顶、归档的留在原处。

**templates**：`title`（最多 60 字）、`category`（最多 20 字，空 = 未分类）、`body`（`{{名称}}` 是待填处）、`pinned_at`、`last_used_at`（只排「/」菜单，不进撤销）。**presets** 用途预设：`name`（必填，最多 30 字）、`content`。**global_instructions**：一行 `content`。

### 消息和调用

**messages**：`session_id`、`run_id`、`parent_id` 上一条、`active_child_id` 显示的下一条、`role`（`user` / `assistant`）、`content`、`reasoning` 思考过程（不发回模型）· `reasoning_ms`、`state`（`complete` / `streaming` / `cancelled` / `failed` / `interrupted`）。同一条的几个版本 = 同一个上一条下、角色相同的几条。

**runs**（每次调用模型）：`session_id`、`kind`（`reply` / `summary` 总结 / `reading` 分段阅读）、`model`（`local:<key>` / `cloud:<id>`）· `model_name`、`state`（`running` / `completed` / `cancelled` / `failed` / `interrupted`）、`error`、`prompt_tokens` · `completion_tokens` · `context_window`、`started_at` · `ended_at`、`skills` 这一问要先用的技能（重新生成、改问题时照抄）。

**context_summaries**：`session_id`、`through_message_id` 摘到哪条、`run_id`、`content`；只存写完的（`context.rs` 开头）。

**run_queries**（查了什么）：`run_id`、`position`、`round`、`call_id` · `tool` · `arguments`、`module`、`label`（「查了」那一行）、`state`（`ok` / `blocked` / `failed` / `unsupported`）、`result` 交给模型的原文、`records` 列到的记录。当前页面也存一行（`call_id = page`、`position = -1`）。查数据的说法（`data.rs`）：「拖着」= 没截止日期、加了 14 天以上没做；「没动的目标」= 进行中、14 天里没完成任何东西；资产排名按期末汇率折成人民币比。

**run_sources**（出处）：`run_id`、`n` 编号、`kind`（`passage` / `record` / `web`）、记录的 `module` · `record_kind` · `ref_id` · `parent` · `day`、段落的 `artifact_id` · `segment_ordinal`、知识库的 `kb_source` · `kb_path`、网页的 `url`、PDF 的一页的 `pdf_page`（原文件的物理页码）· `pdf_label`（页上印的页码）· `pdf_rects`（pdf_find 找到的字的框，JSON `[[x, y, 宽, 高], …]`，PDF 坐标：pt、左下原点、带 CropBox 偏移）、`title`、`detail`、`content` 当时给模型的文字。

**run_passages**：一次回复检索到的段落（当时的文字和标签；知识库的另记 `kb_source`、`kb_path`、`kb_library` 当时的库名）。**run_memories**：用到的记忆（当时的文字和日期）。

**run_asks**（问我的卡片）：`run_id`、`questions` `{ question, kind: single / multi / judge, options }`、`answers` `{ picked, other }`（答了才有）、`state`（`waiting` / `answered` / `cancelled` 没回答）、`answered_at`。

**cards**（确认卡片，一条回复一张）：`run_id`、`state`（`pending` / `confirmed` / `cancelled`）、`decided_at`。**card_items**：`position`、`tool`、`module`、`title` 一行摘要、`fields` 各项（名称、改前、改后）、`input` 确认时交给模块的操作、`checked`、`record_kind` · `record_id` · `record_parent` · `record_day` 写进去的是哪条、`options` · `chosen` 候选和选了哪条、`record_before` 改之前的指纹（记录不见了或变回原样就显示「已撤销」）。Rater 的题：`module` · `record_kind` 是 `rater`，`record_id` 是 `rater_questions` 的行号，`input` 是那道题（新题版本号 0）。

### 附件、资料、记忆

**artifacts**（`youzi-files/` 里的副本；不进撤销）：`display_name`、`file_name` 随机名、`media_type`、`byte_count`、`sha256`、`page_count`（PDF 页数、PowerPoint 张数）、`text_chars`、`row_count` 表格行数、`source_url`（网页，副本是正文 Markdown）、`source_path`（点选、拖入的原文件；同样内容后加的换成它的位置）、`deleted_at`（副本删了，行留着）。

**artifact_segments**：取出的文字：PDF、PowerPoint 每页一段，文本按 Markdown 标题分节；`page_start` · `page_end` · `heading` 写「第 3 页」「某一节」。

**message_attachments**：问题带的附件（`position`）。**reading_notes**：分段阅读时每页 / 每节的要点，属于那次 `reading` 调用，停了也留着。

**project_files**：`project_id`、`current_version`、`indexed` 建立索引。**project_file_versions**：每版一份副本；删资料不删副本。**message_project_files**：问题用了哪些资料、第几版。

**memories**：`content`（1～300 字）、`project_id`（空 = 全局）、`enabled`、`local_only`、`source`（`manual` / `command` 对话里 `/记住` / `import`）、`source_session_id` · `source_message_id`（不设外键）、`noted_on` 记于（改了文字记成当天）。**memory_projects**：同时用于的其他项目。

### 知识库、改文件

**kb_libraries**：`name`（必填，最多 30 字，不重名）。**kb_sources**：`library_id`、`kind`（`folder` / `web`）、`path` 文件夹的完整路径、`artifact_id` 网页的正文副本（附件库里不列）、`cloud`（默认能发）；同一个库里文件夹、网页不重复，文件夹不能互相套，不能是 Pomelo 的数据文件夹。**session_libraries** / **project_libraries**：对话、项目选的库。删库连它的文件夹、网页和选它的记录一起删，原文件不动。

**message_edit_files**：旧问题用「改文件…」选的文件（不再新加）：`path`、`name`；改问题、重新生成时照抄。

**file_edits**（一条回复的卡片：改一个文件，或新建 Word、Excel；删对话时一起删，不进撤销）：`run_id`、`path`（新建的：要存到的文件夹，存好后是文件）、`name`、`kind`（`markdown` / `text` / `word` / `excel` / `new-word` / `new-excel`；新建的内容在唯一一行）、`sha256` 读时的指纹、`total` 共几段、`state`（`pending` / `written` / `cancelled`）、`error`、`decided_at`。**file_edit_items**：每段一行：`paragraph` 第几段（从 1 起）、`before`、`after`（空 = 删掉；Word 的是带格式标记的文字，空一行是这段后面加的新段落）、`checked`。Excel 每处一行：`paragraph` 是位置编号、`label` 哪张表哪格或哪几行、`before` · `after` 给我看的、`payload` 改动（JSON，`excel.rs` 的 `Op`；新建的是每张表的行）。

### 定时、简报、年度总结

**schedules**：`name`（必填，最多 30 字）、`prompt`（必填，最多 2000 字；年度总结的面板里不显示）、`every`（`day` / `week` / `month` / `year`）、`day`（每周 1～7；每月、每年 1～31，没有那天就是最后一天）、`month`（1～12）、`at`（`HH:MM` 本地时间）、`model`（`local:<key>` / `cloud:<id>`，空 = 跟柚子）、`thinking`（默认开）、`effort`（空 = 模型的默认）、`enabled`、`builtin`（`weekly` / `monthly` / `yearly`；删了不再加回）、`keep_chat`（只年度总结用，默认关；关着时写成后删对话，`year_atlas.run_id` 变空）、`since` 从哪一刻起算（新建、改时间、重新打开时设）。**schedule_state**（不进撤销）：`last_slot` 上次跑到的时间（`YYYY-MM-DD HH:MM`）。

**briefs**（今日简报，每天一行；不进撤销和导出）：`day`、`content`、`state`（`writing` / `done` / `failed`）、`error`、`model` · `model_name`、`left_out` 没读的模块、`mood`（`calm` / `busy` / `good` / `worry` / `plain`，从最后一行「心情：…」取出，正文不留）。退出时还在写的，下次重写（其余见 `brief.rs` 开头）。

**month_summaries**（每月小结，只给年度总结）：`month`、`content`、`fingerprint`（读的记录和写法的指纹，没变就直接用）、`model` · `model_name`。**run_month_notes**：一次年度总结用到的每月小结（抄一份）。**year_runs**：一次年度总结写的哪一年。后两张删对话时一起删。

**year_atlas**（Atlas 的「呈现结果」，细则在 `youzi/atlas.rs` 开头）：`run_id`（删对话后变空，行留着）、`year`、`until`（今年没过完时写到哪天，空 = 全年）、`letter` 信的一份、`snapshot` 当时能读的数字和记录（能引用的记录用 `note:12` 这样的键带原文）、`scenes` 每一幕的话和引用的键（「和柚子聊的事」另有 `topics`）、`error` 各幕没写成的原因、`model` · `model_name`、`design` 照哪版设计（如 `tabloid@1.1.1`；空 = `tabloid@1.0.0`）。每运行一次多一行；只有 Atlas 的页能删（`atlas_delete`）。JSON 后来加的键旧行没有，读时当空，库的版本不变。

## 搜索索引（search.db）

从数据算出来的（`search/store.rs` 开头），不进备份、导出和撤销，版本不对就重建；芝麻开门不进来。

- **docs** · **docs_fts**：每条能搜的记录一行（各模块的记录、建出来的模块的条目和课节、柚子的对话标题和每条消息）和按字找的全文索引。**vectors**：记录分段的向量（按向量模型分开，8 位整数）。
- **file_indexes** · **file_chunks**：项目资料的分段和向量（只留「建立索引」的资料的当前版本）。**memory_vectors**：记忆的向量。
- **kb_docs** · **kb_chunks**：知识库文件夹里每个能读的文件（网页算一个）和它的分段、向量。**待更新** = 文件变了，或不是当前向量模型算的；读不了的记下原因，文件再变才重试。扫描件（PDF 的页都在等本机识别、或什么都没认出）也记在 `error`（`scans.rs` 的 `WAITING`、`NOTHING`），按扫描件算、不算读不了；`sha256` 是分段从什么来的：PDF 用了识别出的页时是「文件的 sha256+ocr页数」。本机识别的页，分段的 `label` 是「第 3 页 · 本机识别」。
- **meta**：`rules` 文字规则的版本、`model` 当前向量模型、`rebuild` 正在重建。

## 知识库扫描件的识别结果（ocr.db）

从知识库文件夹里的 PDF 算出来的（`youzi/scans.rs` 开头），不进备份、导出和撤销，版本不对就删掉重建（扫描件再识别一遍）。

- **pages**：某份文件（按内容的 sha256）第几页识别出的字（空 = 没认出字）。文件改了内容就是另一份，旧的留着不碍事。
- **queue**：还要识别的知识库文件（来源编号、文件夹里的相对路径、排队时的 sha256，按 `seq` 先后）。识别完、文件不见了或改了就移出。

## DevJanitor 的扫描结果（janitor.db）

从磁盘算出来的（`youzi/assistants/janitor/store.rs` 开头），不进备份、导出和撤销，版本不对就删掉、下次扫描重建。

- **nodes**：文件夹树，只存够大能画的（`scan::KEEP`；`path` 是 `/` 的是整块磁盘，`kind` `system` = 系统和其他），带大小、文件数、最新修改时间、几处没权限。
- **items**：`listed` 0 一键清理的条目（`kind` 是类别）/ 1 只列出来的（`kind` 是哪一组）；`paths` 要动的路径（.DS_Store、Office 的临时文件一类一条：`paths` 是个人文件夹，文件在 `extra.files`）、`size` 能腾出多少（硬链接、克隆共用的只算一次）、`project_newest` 项目里可重建目录以外最新的文件、`extra` 其余的。
- **meta**：`summary` 扫描摘要（时间、磁盘、废纸篓、Docker、权限）；`envs` 开发环境（`janitor/env.rs` 的 `Env`，`class` 是环境页的哪一类）。
- **backup_seen**（备份）：每个任务（清单里勾上的「下载」文件夹）上次备好时电脑上那个文件夹里有什么（相对路径、大小、修改时间），用来分「电脑上刚删的」和「早就只在硬盘上的」。扫描不碰它；丢了只是少了这个对照。

DevJanitor 在 `youzi.db` 里的：

- 偏好（不进撤销）：`assistant.janitor`（不进一键清理的类别、很久没动的天数（默认 30）、Agent 的模型和思考 `agent`、给自带类别起的名字 `names`：类别编号 → 名字）；`janitor.skill` 改过的 SKILL（空 = 自带的）；`janitor.chat` Agent 的对话（`sessions` 里私密的一行）。
- **janitor_rules**：我加进一键清理的 Agent 规则：`name`、`pattern` 路径（`*` 是一个文件夹名里的任意字）、`recognize` 怎么认、`rebuild` 怎么重建、`checked` 默认勾不勾。
- **janitor_cards**（一条回复里的卡片，跟着 `runs` 删）：`kind`（`trash` / `command` / `rule`；备份的 `move`，以前的 `drive_trash` / `task` 点了只说没有了，见 `backup/cards.rs` 开头）+ `body` 卡片上写的（`janitor/agent.rs` 开头；备份的带 `drive` 硬盘的 UUID）；`state`（`pending` / `running` / `done` / `failed` / `stopped` / `cancelled`）；`result`（每项移没移、命令最后一段输出和退出码、加的规则编号）。执行不进撤销，加规则进。
- 备份（不进撤销）：偏好 `assistant.janitor.backup`（用哪块移动硬盘：卷的 UUID 和名字；`picked` 清单里勾上的最上一层文件夹的名字（硬盘上的或 Downloads 里的；1.1.1 之前的设置没有它，插上硬盘时由旧的任务和 `skipped` 算出来：除了 `skipped` 的都勾上，再加上任务的「下载」文件夹，之后 `skipped` 清空）；任务由清单算出来（勾上的 Downloads 文件夹各一个）：`id`（同一个文件夹一直用同一个）、`source` Downloads 里的文件夹、`target` 硬盘上同名的文件夹（1.1.3 之前还有 `excludes`，读到就丢掉）；`pikpak`：登录的账号（只为显示，登录本身在 rclone 的加密配置里）、`folder` PikPak 上的文件夹（空 = 硬盘的名字）、`limit` 限速 MB/s（0 = 不限））；**backup_runs** 每次备份一行：`dest`（`drive` 往硬盘复制 / `pikpak` 同步到 PikPak；PikPak 的只在传了、挪了或没成时留一行，`files`、`bytes` 是传的，`tasks` 是同步的文件夹（编号 0、只有名字），在 PikPak 上挪了、挪进 ARCHIVE 的个数写在 `note`）、`drive` 硬盘名、`status`（`running` / `done` / `stopped` / `failed`；App 关掉时还在跑的下次打开记成 `stopped`）、`tasks`（每个任务复制了几个、多大、更新了几个、备完没有）、`files`、`bytes`、`errors`（没成的文件和原因，最多 200 条）、`note`、开始和结束时间；「备份」页能删一行（不进撤销）。偏好 `assistant.janitor.backup.look`：柚子上次自动看一眼（线索的指纹、那条回答的编号、时间、没看成的原因）。

## 数据文件（导出 / 导入）

JSON，格式版本 43（`transfer.rs` 的 `FORMAT_VERSION`）。旧格式都能读：缺的模块不动，缺的字段按默认（`transfer.rs` 开头和各模块 `serde` 的默认值，有测试）。

| 部分 | 内容 |
|---|---|
| `format` · `formatVersion` · `exportedAt` · `appVersion` | `pomelo-export`、格式版本、导出时间、App 版本 |
| `kinds` | 各模块的类型名称，按顺序 |
| `modules` | `books` · `films`（含 `rereads`）、`games`（`platform` 写平台名，`replays` 二周目起）、`drawing`（课程套课节）、`packing`（模板和出行各套物品）、`todo`（分组套目标和任务，任务套子任务和 `logs`，转自笔记的带 `fromNote`）、`finance`、`jobs`（状态列表和投递，投递套 `steps`）、`notes`（引用写成对方的 `createdAt`；`tags` 是标签设置）、`days`（生日套 `gifts`）、`custom`（每个建出来的模块：模板、名称、图标、叫法，套字段和条目（值按字段名存），或类型列表和课程、课节（课节的 `kind` 写类型名））、`bookmarks`（文件夹套 `children`，网址带 `url`、`onHome`）、`articles`（正文、置顶、归档、`trashedAt`、附件；加密的正文空，带 `sealed`（十六进制密文）和加密前的 `title`、`tags`；外层的 `tags` 是标签设置） |
| `youzi` | `globalInstructions`、`projects`、`presets`、`templates`、`memories`（用项目名称关联）、`schedules`（不带写的对话）、`seeker`（文件夹和设置、中英草稿、资料和历史、导出记录和岗位关联、岗位的公司资料）、`rater`（题库和历史、卡片练习、面试的准备、复盘和总结；不带没记完的复盘） |

- 记录之间用**名称**关联，不用编号；顺序就是列表顺序；带创建、修改时间。笔记、任务、目标、文章的附件写成 `attachments: [{ name, sha256 }]`：导入时有同样内容的文件就挂上，没有就记成「不在这台 Mac 上」；文章挂上的文件名字不同时，正文里的名字跟着改。加密的文章导到别的芝麻开门里打不开。
- 导入时文件里有哪个模块就替换哪个（连同它的类型），没有的不动；有一条不合格就整个不导入。`custom` 整个替换，编号重新分配。
- 柚子：全局指令、模板、记忆、定时任务整个替换（定时任务按名称对上原来的、写的对话还连着，从导入那一刻起算，选的云端模型这里没有就改成跟柚子）；项目和预设按名称更新或新增。Seeker、Rater 各自整个替换，文件里没有的那部分不动；文件位置只是文字，导入时不读、不建文件。投石问路导入后岗位编号会变，对不上的关联留着快照，要重新关联。
- 不在数据文件里：设置和偏好、芝麻开门、知识库、对话和附件（柚子自己导出）。

## 版本记录

| 版本 | 内容 |
|---|---|
| 1～7 | 设置、书影、类型表、绘画、出门走走 |
| 8～11 | 一步一步：分组、目标、任务；任务类型和完成记录；日期简化；打卡时间段 |
| 12～15 | 钱来钱往、投石问路、念念不忘、芝麻开门 |
| 16～24 | 去掉「已放弃」、书影只记年、余额和持仓按天、投递时间线、标签置顶和图标、重读重看、芝麻开门的排序和分类 |
| 25～30 | 附件、来日方长、五花八门和循序渐进、随意画画去掉待标记、笔记转待办、五花八门的年份字段和课节类型 |
| 31～35 | 导入账单：退款、账单和行、来源、关联决定、柚子做过什么、排序和改名 |
| 36～38 | 标签拖动排序；`picky` 改成 `youzi` |
| 39 | `bookmarks` |
| 40 | 最外层的网址收进新文件夹「未分类」 |
| 41 | `todo_groups.icon`、`todo_goals.icon` |
| 42 | `articles`、`article_tags`、`article_tag_meta`；`attachments.owner` 加 `article` |
| 43 | `games`、`game_replays`；类型 `games`、`game_platform` 的默认值 |

**youzi.db**

| 版本 | 内容 |
|---|---|
| 1～5 | 对话、消息版本树、调用记录；项目、模板、预设、指令、摘要；附件、项目资料、索引；记忆 |
| 6～11 | 查了什么、出处、确认卡片、问题要用的技能；索引搬去 `search.db` |
| 12～15 | 定时任务、今日简报、每月小结、私密对话、思考开关 |
| 16～19 | 更多文件、知识库、改文件、附件的原文件位置 |
| 20～23 | Seeker：设置、草稿、资料和历史、导出和岗位关联；简报的心情；公司资料；英文简历 |
| 24～31 | Rater：题库、卡片练习、面试、复盘 · 记录；自己拖的顺序；不留简历副本；`seeker_groups` |
| 32～35 | 思考强度（对话、定时任务）；`run_asks`；Excel 工具箱 |
| 36～38 | 改名柚子（提示词和模块键）；`year_atlas` |
| 39 | `janitor_rules`、`janitor_cards` |
| 40 | 定时任务写的对话并进「最近」（`manual_lists` 去掉 `scheduled`） |
| 41 | `year_atlas.design` |
| 42 | 每月回顾的内置提示词加「通关的游戏」（只改没改过的） |
| 43 | `schedules.keep_chat` |
| 44 | `backup_runs` |
| 45 | `run_sources` 加 `pdf_page`、`pdf_label`、`pdf_rects`（1.2.0） |

**search.db**：1 搜索索引；2 知识库 `kb_docs`、`kb_chunks`；3 柚子的记录改记 `youzi`。**ocr.db**：1 识别出的页和队列。**janitor.db**：1 扫描结果；2 `backup_seen`。**数据文件**（各版本见 `transfer.rs` 开头）：40 分组和目标带 `icon`；41 `articles`；42 `games`；43 定时任务带 `keepChat`。
