# 数据结构

`pomelo.db` 共 42 张表，当前版本 30；小拾的数据在单独的 `picky.db`（版本 19），搜索索引在 `search.db`（版本 2）。每个版本改了什么见文末。规则都在 Rust 里，各模块文件开头有摘要。

## 通用约定

- 每条记录都有 `id`、`created_at`、`updated_at`，下面省略。
- 日期写成 `2026-09-28`，年月 `2026-09`，年 `2026`。选项类字段存英文代码，界面显示中文。
- `sort_order` 是手动排序（越小越靠前），新加的排最后。
- 文字去掉首尾空格，空文字存为「没填」。删除上一级时下面的一起删（另有说明的除外）。

## 状态、类型、设置

书单、影单、绘画课节共用一套状态（随意画画和循序渐进没有「待标记」）：

| 代码 | 摸鱼读读 | 好奇看看 | 随意画画 |
|---|---|---|---|
| `unmarked` | 待标记 | 待标记 | — |
| `todo` | 想读 | 想看 | 未开始 |
| `doing` | 在读 | 在看 | 进行中 |
| `done` | 已读 | 已看 | 已完成 |

- 新建书影默认「待标记」，新建课节默认「未开始」。
- **完成时间**：书影只记年，课节记年月。标为完成时还没填就自动填今年 / 当月；改回别的状态不清除（课节圆圈上的「取消完成」例外，相当于撤销）。

**kinds**（能自己增删改的类型）：`module`、`name`（同模块不重名，最多 20 字）、`sort_order`。记录里只存 `kind_id`，删类型后记录变「未分类」。每个循序渐进模块各有一组（`progress/编号`，一开始是空的）。各模块：`books`（虚构、非虚构、漫画、其他）、`films`（电影、剧集、动漫）、`drawing`（正课、DLC 跟画、LIVE 辅导）、`packing`（重要物品、随身、护理、兴趣）、`fin_out` 支出分类、`fin_in` 收入分类、`job_channel` 投递渠道。

**settings**：`key`、`value`（JSON），界面状态（`ui.module_hidden` 隐藏的模块、`ui.module_order` 侧栏顺序、`ui.sidebar_collapsed` 等）和开机启动这类开关。不进撤销。

## 附件

规则见 `modules/attachments.rs` 开头。

- **attachment_files**：`name` 原文件名、`stored` `attachments/` 里副本的名字（随机名；空 = 不在这台 Mac 上，比如从别处导入的）、`kind`（`image` / `file`，按扩展名）、`bytes`、`sha256`。同样的内容只存一份，最大 200 MB。存副本不进撤销。
- **attachments**（谁挂了哪个文件）：`owner`（`note` / `task` / `goal`）、`record_id`、`file_id`、`sort_order`；一条最多 50 个。和记录一起保存（一步 ⌘Z）；记录删掉时它的这几行一起删，撤销会一起回来。不设外键：清理掉的文件被撤销带回来时显示「不在这台 Mac 上」。
- **清理**：没有记录用到的副本连文件一起删，不能撤销。

## 摸鱼读读 · 好奇看看

**books** / **films**（结构相同，影视里作者叫导演）：`title`（必填）、`author` / `director`（多人用「 / 」隔开）、`country`、`kind_id`、`status`、`rating`（1～5，可不填）、`finished_year`、`note`。

**book_rereads** / **film_rereads**（第一次之后的每一次）：`book_id` / `film_id`、`year`、`rating`、`sort_order`。表格的完成时间 = 所有次里最近的一年。

## 随意画画

**drawing_courses**：`name`（不重名）、`note`、`sort_order`。**drawing_lessons**：`course_id`、`code` 编号（如 `2B`，只用来显示）、`kind_id`、`title`（必填）、`status`、`finished_month`、`note`、`sort_order`。

## 出门走走

**packing_templates**：`name`（不重名）、`note`、`sort_order`。**packing_template_items**：`template_id`、`kind_id` 类别、`name`、`note`、`sort_order`。

**trips**：`name`、`template_id`（只作记录，模板删了出行还在）、`depart_on` 出发日期、`note`。**trip_items**：`trip_id`、`kind_id`、`name`、`note`、`packed` 已装好、`sort_order`。建出行时复制一份清单（从出行复制时全部变回没装），之后各改各的。

## 一步一步

**todo_groups**：`name`（不重名）、`sort_order`。

**todo_goals**：`group_id`、`name`、`status`（`active` / `paused` / `done`）、`start_date` · `end_date`、`note`、`review` 复盘、`sort_order`。

**todo_tasks**：

| 字段 | 含义 |
|---|---|
| `group_id` · `goal_id` · `parent_id` | 所在分组、目标（可空）、上一层任务（空 = 顶层；子任务跟着顶层任务的分组和目标） |
| `title` · `note` · `link` | 标题、内容（可以 `**加粗**`）、链接 |
| `from_note` | 从哪条笔记转来的（笔记删了变空，任务留着） |
| `task_type` | `normal` 普通 / `repeat` 重复 / `checkin` 打卡 / `progress` 进度条 |
| `start_date` · `due_date` | 时间段（开始可空）；重复任务的 `due_date` 是「这一次」，由规则推算 |
| `plan_date` | 重复（完成后）：从哪天开始 |
| `repeat_rule` · `repeat_total` · `done_before` | 重复 / 打卡的规则、一共几次（空 = 不限）、用 Pomelo 前已经做了几次 |
| `target` · `unit` | 进度条的目标数量、单位 |
| `done` · `done_at` · `collapsed` · `sort_order` | 完成、完成时间、子任务折叠、同层顺序 |
| `repeat_until` · `period` | 旧字段，不用 |

**todo_logs**（完成记录）：`task_id`、`day`、`amount`（默认 1）。重复每做完一次、打卡每天、进度条每记一笔各一条。

**重复规则**：`daily`、`weekly:1,2,3,4,5`（周一是 1）、`monthly:1,15`（没有这天用月底）、`yearly:12-31`（`/3` 每 3 年；02-29 平年用 02-28）、`every:3d` / `every:2w`、`after:7d` · `2w` · `6m` · `1y`（完成后过多久）。打卡只能每天或每周几天。

- **勾选**：勾上一个任务，下面的一起勾上；子任务全勾上时上一层自动勾上；取消或新加子任务时上一层自动取消。进度只数最底层的任务。
- **今天**：没完成、不在暂停或已完成的目标里，截止是今天或更早、或今天在时间段里。
- **重复**：从今天（或开始日期，取晚的）起找第一个该做的日子；做完一次记一条、挪到下一次（一定在这一次和做完那天之后）；次数做满或过了结束日期就整个完成。「跳过这次」只挪日期。已经做了几次 = `done_before` + 记录条数。
- **打卡**：时间段里该打的都打了马上完成（没开始日期的从添加那天算），取消一天就变回没完成；补打只能在时间段里、今天以前。时间段结束也自动完成（例行检查）。
- **进度条**：累计达到目标自动完成。

## 钱来钱往

金额都存成「分」的整数（持仓单价除外）。币种 `CNY` / `SGD` / `HKD` / `USD`，从不直接相加，只有总资产按汇率折成人民币。

| 表 | 字段 |
|---|---|
| **fin_accounts** 账户 | `name`（不重名）、`kind`（`bank` / `wallet` / `broker`）、`currency`（记过余额或持仓后不能改）、`closed` 停用、`note`、`sort_order` |
| **fin_balances** 余额 | `account_id`、`day`、`amount`（同账户每天一条；券商账户的余额就是总值） |
| **fin_rates** 汇率 | `day`、`currency`、`rate`（1 单位合多少人民币） |
| **fin_holdings** 持仓 | `account_id`（只能券商）、`day`、`name`、`quantity`（> 0）、`price` 单价（按元存）、`sort_order`；保存时替换那一天的全部持仓 |
| **fin_entries** 收支 | `flow`（`out` / `in`）、`amount`（> 0）、`currency`、`day`（时间段的第一天）、`end_day`（最后一天，一天的不填）、`kind_id`、`fixed_id` 由哪个固定支出记的（删固定支出后留着）、`note` |
| **fin_fixed** 固定支出 | `name`（不重名）、`flow`、`amount`、`currency`、`kind_id`、`rule`（`monthly:15` / `yearly:03-01`）、`next_day` 下次哪天记、`stopped`、`note`、`sort_order` |
| **fin_budgets** 预算 | `kind_id` 支出分类、`currency`、`amount` 每月（同分类每币种一条） |

- **总资产**：每个记过余额的日子算一次：各账户到那天为止最近的余额 × 各币种到那天为止最近的汇率。停用的账户过了它最后一次余额就不算。缺某币种的汇率时那天不算总数，并提示缺什么。
- **时间段**（记小额）按天平摊，跨月的按天数分到各月（四舍五入到分，加起来正好等于金额）；月合计、报表、预算都这样算。
- **固定支出**：到了 `next_day`（例行检查或刚保存时）就记一笔（备注写名称）、挪到下一次，漏掉的一起补。
- **预算**：按「分类 + 币种」比当月这个币种的支出。

## 投石问路

**job_statuses**：`name`（不重名）、`stage` 阶段（`wish` 准备 / `applied` 等回音 / `active` 进行中 / `got` 拿到 / `closed` 结束）、`sort_order`。阶段决定规则，界面不显示、不能改；新加的都是进行中。有投递正处在、或时间线上有它的状态不能删；时间线上有它的不能改成准备；至少留一个。

**jobs**：`company`（必填）、`position`、`location`、`link` JD、`kind_id` 渠道、`resume` 简历位置（家目录里的存成 `~/…`）、`note`、`followed_on` 最近一次「已跟进」。

**job_steps**（时间线）：`job_id`、`status_id`（不能是准备阶段；同一条投递不重复）、`done`、`done_on`（可以不填）。按状态列表的顺序显示。

- **现在的状态**：最后一个勾上的；一个没勾 = 第一个准备阶段的状态。投递日期 = 最早勾上的那天。都不存，每次算。
- **新建投递**的时间线默认是除了准备、结束阶段的所有状态，都不勾。
- **该跟进**：现在在等回音阶段，离那一步的日期（或最近一次跟进，取晚的）满 14 天。
- **漏斗**：只看至少勾了一个的投递；等回音、进行中的状态，加上第一个拿到阶段的状态，各写「勾上的 / 时间线上有它的」。
- **统计**都由 `jobs.rs` 的 `stats` 算，统计页、首页和小拾共用。

## 念念不忘

| 表 | 字段 |
|---|---|
| **notes** | `content` 正文、`pinned` 置顶、`created_at`（精确到毫秒、不重复，导出时靠它认出是哪一条） |
| **note_tags** | `note_id`、`tag` 完整路径（如 `读书/摘抄`）；每次保存都从正文重新提取 |
| **note_refs** 引用 | `note_id` 引用别人的、`ref_id` 被引用的、`sort_order`；任何一边被删引用就没了 |
| **note_tag_meta** 标签设置 | `tag`、`pin_order` 置顶先后（只有第一级标签能有）、`icon`（`i:book-open` 这样的是线条图标，别的是 emoji，最多 16 个字符） |

- **标签写法**：`#` 在开头或空格后，到空格或下一个 `#`；结尾的 `/` 不算；最多 50 字，不能以 `/` 开头、不能有 `//`。按标签看包括下一级；标签树上的数字含下一级。
- **改名**：把正文里的 `#旧名`（连同 `#旧名/…`）换成新名字、重新提取；标签设置跟着搬（合并时目标自己的图标优先，置顶取早的；改成子标签就不再置顶）。标签和设置都空了就删掉那行。
- **置顶的标签**：只列还有笔记用到的第一级标签，按 `pin_order`。
- **正文格式**（只影响显示）：`**加粗**`、`==高亮==`、`++下划线++`、`- ` / `1. ` 列表（每多两个空格深一级）、网址。每一行是一段。
- 置顶笔记只在没有筛选时排最前；改置顶不更新修改时间。哪天记的按本机时间算。

## 来日方长

规则见 `modules/days.rs` 开头。

| 表 | 字段 |
|---|---|
| **days** | `kind`（`day` 纪念日和倒数日 / `birthday` / `document` 证件）、`name`、`date`（`2026-10-17`；不知道年份的生日写 `--10-17`）、`repeat`（`none` / `year` / `month` / `week`，生日总是每年、证件总是不重复）、`remind` 提前几天（`30,7,0`，大的在前，最多 5 个，0 是当天）、`pinned`（最多一条）、`note`、`ideas` 礼物点子（只有生日） |
| **day_gifts** 送过的 | `day_id`、`year`、`gift`、`sort_order`（只有生日） |

下一次、还有几天、第几周年 / 几岁、快到了没有都不存，每次按今天算。置顶一条会取消另一条；只改置顶不更新修改时间。

## 五花八门 · 循序渐进

规则见 `modules/custom.rs`、`lists.rs`、`progress.rs` 开头。

| 表 | 字段 |
|---|---|
| **custom_modules** | `template`（`list` / `progress`）、`name`（不重复，最多 20 字）、`icon`（24 个之一）、`course_word` · `lesson_word` 两层的叫法（只有循序渐进）。编号永远不重用 |
| **list_fields** 字段 | `module_id`、`name`（同一个清单里不重复，不能叫「名称」）、`type`（`text` / `longtext` / `number` / `year` 年份 / `select` / `multi` / `check` / `rating` / `link`，建好不能换）、`options` 选项（JSON，最多 50 个）、`shown` 表格里显示、`sort_order`。一个清单最多 20 个 |
| **list_items** 条目 | `module_id`、`name`、`data`（`{"字段编号": 值}`，空的不存：单选存选项名，多选存选项名的列表，勾选只存「是」，评分 1～5，年份存 `"2026"`（1900～2100））、`sort_order` |
| **progress_courses** | `module_id`、`name`（同模块不重复）、`note`、`sort_order` |
| **progress_lessons** | `course_id`、`code`、`kind_id` 类型（这个模块自己的类型，`kinds.module` = `progress/编号`；删了类型变空 = 未分类）、`title`、`status`（`todo` / `doing` / `done`）、`finished_month`、`note`、`sort_order` |

每个模块的键是 `list/编号` 或 `progress/编号`，侧栏顺序、隐藏、「发给云端」、搜索结果、循序渐进的类型列表都按它。删模块时里面的一起删（连同它的类型）；删字段时每条里它的值去掉；选项改名时每条跟着改。都是一步 ⌘Z。

## 芝麻开门

数据库里只有密文。规则见 `modules/vault.rs` 开头，安全设计见[技术架构](architecture.md)。

- **vault_meta**（一行）：Argon2id 的盐、内存、轮数，`wrapped_key` 锁起来的密码库钥匙、`key_check`、`master_at` 上次输主密码的时间、`touch_build` · `touch_account` Touch ID 那份是哪个版本存的、在钥匙串哪一项、`touch_locked` 要先输主密码。不进撤销。
- **vault_items**：`data`（名称、链接、用户名、账号、密码、备注合在一起加密）、`sort_order`、`category_id`（删了分类变空）、`archived`。能撤销。全是 0（没拖过）时按名称排；拖过就按 `sort_order`，之后新加的放最后。归档不改「修改于」。
- **vault_categories**：`data`（分类名，加密，不重名，最多 20 字）、`sort_order`。能撤销。
- 密码库钥匙以后不变，改主密码只换锁。Touch ID 那份只认写它的那个版本，所以更新后先输一次主密码。不进数据文件；备份里是密文，恢复较早的备份后要用那时的主密码。

## 首页

没有自己的表，每次打开由 `home.rs` 现算，隐藏的模块不算。

- **今年读完 / 看完**：已读 / 已看且完成时间是今年的，加上重读重看里有今年的，一本只算一次。**画完**：今年完成的课节。**完成**：同「回看」：今年勾掉的任务加上今年的重复、打卡、进度记录。**笔记**：今年记的。**投出**：投递日期在今年的。
- **求职**：本周投了 = 投递日期在本周一以后；进行中 = 在等回音或进行中阶段；拿到 = Offer、已接受；该跟进的按等得久的排前面。
- **最近记录**：各类记录按新加的时间、待办按勾掉的时间，取最近 8 条（书影、课节只按新加算）。
- **绘画进度**：有课节、没全部完成的课程；下一节 = 进行中的那节，没有就是第一节没完成的。
- **快到的日子**：到了最早的提醒那天、还没过的，和过期的证件。

## 小拾（picky.db）

- 单独一个数据库，和 `pomelo.db` 放在同一个文件夹，有自己的升级脚本（`migrations/picky/`）和版本号；`pomelo.db` 不因小拾改结构。文件副本在旁边的 `picky-files/`。
- 撤销覆盖小拾的整理操作；发消息、生成回复、建索引、删副本、提议和写文件、定时任务和简报自己跑的都不进撤销。
- 数据文件（导出 / 导入）只带记忆、模板、用途预设、全局指令、项目和背景指令、定时任务；对话和附件用小拾自己的导出。

**preferences**（`key`、`value` JSON，不进撤销；API Key 不在这里，只在钥匙串）：

| 键 | 内容 |
|---|---|
| `model.selection` · `model.last_local` | 现在选的模型（`local` 或 `cloud:<id>`）、上次选的本地模型 |
| `cloud.models` | 常用云端模型（名称、服务、接口模型名、上下文、回复上限、思考能力 `never` / `switch` / `always`，没写的算能开关），顺序就是模型菜单的顺序；第一次打开时放两个 DeepSeek 默认项，删了不再加回来 |
| `data.cloud` | 各模块能不能发给云端（只存改过的；默认钱来钱往不能，其他能） |
| `memory.enabled` | 使用记忆的总开关（默认开） |
| `skills` · `skills.contact` | 每个技能的「启用」「小拾自己决定用」（只存改过的）；书单 / 影单检索的联系邮箱 |
| `brief.enabled` · `brief.model` · `brief.thinking` | 今日简报的开关（默认开）、模型（空 = 跟着小拾）、思考（默认开） |
| `assistant.translate` | 翻译助手的上一次：设置（源语言、目标语言、模型、思考、自己写的提示词、关键词）和原文、译文、怎么结束的、用的模型 |
| `work.folder` | 「保存为 Word」上次存到的文件夹（不在了就是「文稿」） |
| `ui.*` | 上次打开的对话、列表收起、收起的项目、归档展开、开着的助手 |

### 对话和项目

**sessions**：`title`、`title_source`（`auto` 取第一个问题的第一行、最多 40 字 / `user` 改过名；改成空回到自动）、`active_root_id` 显示的路径从哪条消息开始、`project_id`、`pinned_at`、`archived_at`、`sort_order`、`instructions` 对话指令、`preset_id` 用途预设（删预设时清空）、`memory_disabled` 关闭记忆、`thinking` 思考开关（新对话默认开）、`knowledge` 知识库开关（默认关）、`private` 私密、`scheduled` 是定时任务写的（任务删了也还是）· `schedule_id` 哪个任务 · `unseen` 还没打开过（打开就清掉，不进撤销）。`updated_at` 是最后一次发消息的时间，整理操作都不碰它。

- **在哪个列表**：归档的只在「归档」；置顶的只在「置顶」（项目不变）；其他的在所属项目，没项目的：定时任务写的在「定时」，别的在「最近」。
- **顺序**：没手动拖过的列表按最后发消息的时间排；拖过一次就记在 **manual_lists**（`pinned`、`scheduled`、`recent`、`archived`、`project:<id>`），之后照手动的顺序，新来的排最上面。
- 删对话一起删它的消息和调用记录。

**projects**：`name`（最多 60 字，空的叫「未命名项目」）、`instructions` 背景指令、`include_global_memories`（默认开）、`sort_order`（新项目排最上面）。删项目不删对话：它下面的移到「最近」最上面，置顶、归档的留在原处。

**templates**：`title`（最多 60 字）、`category`（最多 20 字，空的算「未分类」）、`body`（`{{名称}}` 是待填位置）、`pinned_at`、`last_used_at`（只用来排「/」菜单，不进撤销）。**presets** 用途预设：`name`（必填，最多 30 字）、`content`、`sort_order`。**global_instructions**：一行，`content`。

- **指令**：系统消息 = 全局、项目、对话、预设四层，空的不要，中间空一行。预览和真正发送用同一段代码。草稿的对话指令和预设随第一句保存。

### 消息和调用

**messages**：`session_id`、`run_id`、`parent_id` 上一条、`active_child_id` 显示的下一条、`role`（`user` / `assistant`）、`content`、`reasoning` 思考过程（不发回给模型）、`reasoning_ms`、`state`（`complete` / `streaming` / `cancelled` / `failed` / `interrupted`）。

- 同一条消息的几个版本是同一个上一条下面、角色相同的几条；显示的路径从 `active_root_id` 顺着 `active_child_id` 走（链接坏了走最新的）。
- 发一个问题：在路径末尾加上问题和一条空的回复，回复边写边存（每 0.75 秒）。打开 App 时上次没写完的标成「中断」。

**runs**（每次调用模型）：`session_id`、`kind`（`reply` / `summary` 总结 / `reading` 分段阅读）、`model`（`local:<key>` / `cloud:<id>`）、`model_name`、`state`（`running` / `completed` / `cancelled` / `failed` / `interrupted`）、`error`、`prompt_tokens` · `completion_tokens`、`context_window`、`started_at` · `ended_at`、`skills` 这一问要求先用的技能（重新生成、改问题时照抄）。

**context_summaries**：`through_message_id` 摘要到哪条为止、`run_id`、`content`。只存写完的；它所到的消息在显示的路径上才用，改了或重新生成前面的消息就不用了。

- **上下文**：先放系统消息、摘要和新问题，再从最近的消息往前放，放不下的不发；回复留窗口的 1/4（1024～8192），开思考时再多留。窗口用本地服务报的值（不超过清单），云端用常用模型里的设置，模拟模式 16384。大小优先让本地模型自己数，数不了就保守估算。
- **总结**：保留最近两轮（短对话一轮）原文，之前的合进摘要；占用生成的位置，能停止。

**run_queries**（查了什么）：`run_id`、`position`、`round`、`call_id` · `tool` · `arguments`、`module`、`label`（「查了」那一行）、`state`（`ok` / `blocked` / `failed` / `unsupported`）、`result` 交给模型的原文、`records` 列到的记录。当前页面也存成一行（`call_id = page`、`position = -1`）。

- **查数据的规则**（`data.rs`）：每个模块一个只读查询，算数用各模块自己的函数；芝麻开门没有查询；隐藏的模块不提供；选云端时不能发的回「没有查」。每次最多约 3000 token。追问时最近 3 次查到的（每次最多约 1500 token）附在当时的问题后面，按现在的规则再筛一遍。
- **几个算法**：资产排名 = 各账户、持仓的变化按期末的汇率折成人民币比；「拖着」= 没截止日期、加了 14 天以上还没做，「没动的目标」= 进行中、14 天里没完成任何东西；绘画速度 = 最近 6 个月画完的节数 ÷ 6，预计 = 剩下的 ÷ 速度向上取整（`drawing.rs` 的 `pace`）；出行可能漏的 = 模板和其他出行里有、这次没有的物品（不分大小写），这次的模板里的排前面（`packing.rs` 的 `maybe_missing`）。

**run_sources**（出处）：`run_id`、`n` 编号、`kind`（`passage` 资料段落 / `record` 记录 / `web` 网页）、记录的 `module` · `record_kind` · `ref_id` · `parent` · `day`、段落的 `artifact_id` · `segment_ordinal`、知识库的 `kb_source` · `kb_path`、网页的 `url`、`title`、`detail`、`content` 当时给模型的文字。先编检索到的段落（项目资料、知识库），再编搜到的记录；同一条记录、同一个网址只有一个号。追问时以前搜到的照样带上，但去掉编号。

**run_passages**：一次回复检索到的段落（当时的文字和标签）；知识库的另记 `kb_source`、`kb_path`、`kb_library` 当时的库名。**run_memories**：用到的记忆（当时的文字和日期）。

**cards**（确认卡片，一条回复一张）：`run_id`、`state`（`pending` / `confirmed` / `cancelled`）、`decided_at`。**card_items**：`position`、`tool`、`module`、`title` 一行摘要、`fields` 卡片上的各项（名称、改前、改后）、`input` 确认时交给模块的操作（确认和试写走同一个函数）、`checked`、`record_kind` · `record_id` · `record_parent` · `record_day` 写进去的是哪条、`options` · `chosen` 找到几条时的候选和选了哪条、`record_before` 改之前那条的指纹（记录不见了或变回原样就显示「已撤销」）。确认时只写勾上的，整张一步 ⌘Z（卡片还是「已确认」）。

### 附件、资料、记忆

**artifacts**（文件副本，在 `picky-files/`）：`display_name`、`file_name` 随机名、`media_type`、`byte_count`、`sha256`、`page_count`（PDF 页数、PowerPoint 张数）、`text_chars`、`row_count`（表格的行数）、`source_url`（网页的网址，副本是正文 Markdown）、`source_path`（从哪个文件加进来的：点选、拖入的才有，后加的同样内容的文件会换成它；Word 工具箱、文本工具箱改的就是它）、`deleted_at`（副本删了，行留着）。同样的内容只存一份；最多 200 MB，要有文字（扫描版 PDF 不行）。不进撤销。

**artifact_segments**：取出的文字：PDF 每页一段，PowerPoint 每页一段，文本按 Markdown 标题分节（每段最多 6000 字），表格只存表头和前 50 行（超过 200 行时）。`page_start` / `page_end` / `heading` 用来写「第 3 页」「某一节」。

**message_attachments**：问题带的附件（`position`）；同一条路径上前面问题的附件，后面提问时还会一起发。**reading_notes**：分段阅读时每页 / 每节的要点，属于那次 `reading` 调用，停止了也留着。

**project_files**：`project_id`、`current_version`、`indexed` 选了「建立索引」、`sort_order`。**project_file_versions**：每一版对应一份副本。同一个项目里同样的内容不能加两次；删资料不删副本。都能 ⌘Z。**message_project_files**：问题用了哪些资料、第几版。

**memories**：`content`（1～300 字）、`project_id`（空 = 全局）、`enabled`、`local_only`、`source`（`manual` / `command` /记住 / `import`）、`source_session_id`（不设外键）、`noted_on` 记于（改了文字就记成当天）。**memory_projects**：同时用于的其他项目。能 ⌘Z；删项目时它的记忆改成全局并停用。

- **哪些记忆能用**：总开关、这个对话、这一条都没关；启用的、在范围内的（不在项目里只用全局的；项目里用本项目的、共享给它的，加上全局的，除非项目关了），选云端时去掉仅本地的。预览和发送同一条规则。分段阅读不带记忆。

### 知识库、改文件

**kb_libraries**：`name`（必填，最多 30 字，不重名）、`sort_order`。删库连同它的文件夹、网页和「选了它」的记录一起删，原来的文件不动。**kb_sources**：`library_id`、`kind`（`folder` / `web`）、`path` 文件夹的完整路径、`artifact_id` 网页的正文副本（附件库里不列）、`cloud` 能不能发给云端（默认能）、`sort_order`。同一个库里文件夹不能重复、不能一个套一个，网页不能重复，不能放 Pomelo 的数据文件夹。**session_libraries** / **project_libraries**：对话、项目选了哪些库。这些都能 ⌘Z。

- **查哪些**：对话的开关开着时，对话选的库，没选就是项目选的，也没选就是全部；用云端模型时去掉 `cloud` 关着的。

**message_edit_files**：以前用「改文件…」选的文件（现在不再新加），`path` 完整路径、`name`；改问题、重新生成时照抄，工具箱也能改它。**file_edits**（一条回复的卡片：改一个文件，或新建一个 Word）：`run_id`、`path`（新建的：要存到的文件夹，存好后是文件）、`name`、`kind`（`markdown` / `text` / `word` / `new-word` 新建的 Word，内容在唯一一行的 `after`）、`sha256` 读的时候文件的指纹、`total` 一共几段、`state`（`pending` / `written` / `cancelled`）、`error`、`decided_at`。**file_edit_items**：每段一行：`paragraph` 第几段（1 开始）、`before`、`after`（空的是删掉这一段；Word 的是带格式标记的文字，空一行是在这段后面加的新段落）、`checked`。删对话时一起删，都不进撤销。

- **段落**：Markdown / 纯文本是空行隔开的块（代码块里的空行不算）；Word 是正文的每个段落，一个表格算一段（不能改），页码目录这类域、内容控件、文本框、空段不能改。

### 定时、简报、年度总结

**schedules**：`name`（必填，最多 30 字）、`prompt` 要做什么（必填，最多 2000 字）、`every`（`day` / `week` / `month` / `year`）、`day`（每周 1～7；每月、每年 1～31，没有那天就是最后一天）、`month`（每年 1～12）、`at`（`HH:MM`，本地时间）、`model`（`local:<key>` / `cloud:<id>`，空 = 跟着小拾）、`thinking`（默认开；模型要做得到）、`enabled`、`builtin`（`weekly` / `monthly` / `yearly`；删了不再加回来）、`sort_order`、`since` 从什么时候起算（新建、改时间、重新打开时设成那一刻）。能 ⌘Z；删任务不删它写的对话。

- **schedule_state**：`last_slot` 上次跑到哪个时间（`YYYY-MM-DD HH:MM`），不进撤销。最近一个该跑的时间比 `last_slot` 和 `since` 都晚就跑，跑之前先记下。

**briefs**（今日简报，每天一行）：`day`、`content`、`state`（`writing` / `done` / `failed`）、`error`、`model` · `model_name`、`left_out` 没读的模块。重写和失败时 `content` 留着上一次的。退出时还在写的，下次算「上次没写完」重新写。不进撤销、不导出。

**month_summaries**（每月小结，只给年度总结用）：`month`、`content`、`fingerprint`（读的记录和写法的指纹，没变就直接用）、`model` · `model_name`。**run_month_notes**：一次年度总结用到的每月小结（照抄一份）。**year_runs**：一次年度总结写的是哪一年。后两张删对话时一起删。

## 搜索索引（search.db）

从数据算出来的东西。不进备份、导出和撤销；删掉、损坏或版本不对就删了重建。芝麻开门不进来。

- **docs**：每条能搜的记录一行（各模块的记录、自己建的模块的条目和课节、小拾的对话标题和每条消息）：标题、所在位置、打开它要的编号和日期、`body` 要搜的文字（不含状态和标签名）、`hash` 管所有字段、`vhash` 只管算向量的那部分。**docs_fts**：按字找的全文索引（中文相邻两个字一组，英文和数字按词开头）。
- **vectors**：每条记录按约 240 个词分段（最多 64 段），每段一个向量，按向量模型分开存，压成 8 位整数（1024 维约 1 KB）。
- **file_indexes / file_chunks**：项目资料的索引，按副本、SHA-256 和向量模型存：约 240 个词一段（短的末句在下一段开头再出现一次），嵌入时带「资料名 · 第 3 页」，最多 5000 段。只有「建立索引」的资料的当前版本保留。**memory_vectors**：记忆的向量（按模型和文字的哈希）。
- **kb_docs**：知识库文件夹里每个能读的文件（网页算一个，`path` 空）：`source_id`、`path`（相对文件夹）、看到的 `size` · `mtime`、段落从什么算的（`sha256`、`model`）、`fresh` 对得上、`error` 读不了的原因（文件再变才重试）。**待更新** = 不 `fresh`，或没出错但不是当前的向量模型算的。**kb_chunks**：`segment`、`label`、`content`、`vector`（8 位整数）；切段同项目资料。
- **meta**：`rules` 文字规则的版本、`model` 当前的向量模型、`rebuild` 正在重建。

## 数据文件（导出 / 导入）

JSON，当前格式版本 18；旧版本的文件都能导入（转换写在 `transfer.rs` 和各模块里）。

| 部分 | 内容 |
|---|---|
| `format` · `formatVersion` · `exportedAt` · `appVersion` | `pomelo-export`、格式版本、导出时间、App 版本 |
| `kinds` | 各模块的类型名称，按顺序 |
| `modules` | `books` · `films`（含 `rereads`）、`drawing`（课程套课节）、`packing`（模板和出行各套物品）、`todo`（分组套目标和任务，任务套子任务和 `logs`，转自笔记的带 `fromNote`）、`finance`、`jobs`（状态列表和投递，投递套 `steps`）、`notes`（引用写成对方的 `createdAt`；`tags` 是标签设置）、`days`（生日套 `gifts`）、`custom`（每个自己建的模块：模板、名称、图标、叫法，套字段和条目（值按字段名存），或 `kinds` 类型列表和课程、课节（课节的 `kind` 写类型名））。版本 17 以前的五花八门有「日期」字段，读成年份（只留年）；循序渐进没有类型 |
| `picky` | `globalInstructions`、`projects`、`presets`、`templates`、`memories`（用项目名称关联）、`schedules`（不带它们写的对话） |

- 记录之间用**名称**关联，不用编号；顺序就是列表顺序；创建、修改时间都带上。笔记、任务、目标的附件写成 `attachments: [{ name, sha256 }]`：导入时有同样内容的文件就挂上，没有就记成「不在这台 Mac 上」。
- 导入时文件里有哪个模块就替换哪个（连同它的类型），没有的不动；任何一条不合格就整个不导入。`custom` 整个替换，编号重新分配。
- 小拾的部分：全局指令、模板、记忆、定时任务整个替换（定时任务按名称对上原来的，写的对话还连着，从导入那一刻起算，选的云端模型这里没有就改成跟着小拾）；项目和预设按名称更新或新增。
- 芝麻开门、知识库不在数据文件里。

## 版本记录

| 版本 | 内容 |
|---|---|
| 1～7 | 设置、书影、类型表、绘画、出门走走 |
| 8～11 | 一步一步：分组、目标、任务；任务类型和完成记录；日期简化；打卡时间段 |
| 12 · 13 · 14 · 15 | 钱来钱往 · 投石问路 · 念念不忘 · 芝麻开门 |
| 16 | 目标去掉「已放弃」（改成暂停） |
| 17 | 书影完成时间只记年 |
| 18 | 钱来钱往：余额、汇率、持仓按天；持仓数量 × 单价；收支时间段；收支和固定支出去掉账户 |
| 19 | 投石问路：每条投递的时间线；去掉截止日期、投递日期、薪资、求职信（写进备注）、和待办的关联 |
| 20 · 21 · 22 | 标签置顶和图标 · 重读重看 · 只能置顶第一级标签 |
| 23 · 24 | 芝麻开门：手动排序 · 分类（名称加密）、归档 |
| 25 | 附件：附件文件和谁挂了哪个文件 |
| 26 | 来日方长：日子和送过的礼物 |
| 27 | 五花八门 · 循序渐进：建出来的模块、清单的字段和条目、进度的课程和课节 |
| 28 | 随意画画去掉「待标记」（原来的变成未开始） |
| 29 | 笔记转待办：任务记下从哪条笔记来 |
| 30 | 五花八门的「日期」字段改成「年份」（原来的日期只留年）；循序渐进的课节加类型 |

**picky.db**

| 版本 | 内容 |
|---|---|
| 1～3 | 设置、对话、消息（版本树）、调用记录；项目、置顶、归档、手动顺序；模板、用途预设、全局指令、对话指令、上下文摘要 |
| 4 · 5 | 附件和分段文字、阅读要点、项目资料和版本、索引、检索到的段落 · 记忆和它用到的地方、项目的「同时使用全局记忆」、对话的「关闭记忆」 |
| 6～8 | 查了什么（`run_queries`）· 索引和向量搬去 `search.db` · 出处（`run_sources`） |
| 9 · 10 | 确认卡片 · 卡片里要选的几条、选了哪条、改之前的样子 |
| 11 | 问题要用的技能（`runs.skills`）、网页出处的地址 |
| 12 · 13 | 定时任务（放进每周、每月回顾）、对话是不是定时任务写的 · 今日简报、每月小结、定时任务能每年（放进年度总结） |
| 14 · 15 | 私密对话、年度总结写的是哪一年 · 思考开关（对话、定时任务，默认开） |
| 16 | 更多文件：网页的网址、表格的行数 |
| 17 | 知识库：库、文件夹和网页、对话和项目选的库、对话的开关；段落和出处记着来自哪个文件 |
| 18 | 改文件：问题要改的文件、回复提的改动 |
| 19 | 附件记下原文件在哪（`artifacts.source_path`，Word 工具箱、文本工具箱用） |

**search.db**：1 是搜索索引；2 加知识库（`kb_docs`、`kb_chunks`）。
