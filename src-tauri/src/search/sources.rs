//! What the index holds: one source per kind of record (芝麻开门 never). Each SELECT yields, by
//! position: ref_id, title, detail, parent, day, at, body.
//! - title / detail / parent / day: what a hit shows and how the page opens it (as in ⌘K).
//! - at: when it was made (ties sort newest first).
//! - body: the searched text (word index, passages for vectors, snippets); it starts with the
//!   title for records, so a title change is found too. Labels and statuses stay out of it.
//! The label prefixes the text sent to the embedding model ("任务：…").
//! - 五花八门 / 循序渐进: one source per kind covers every made module; its `module` ends with "/"
//!   and an 8th column gives the row's own key ("list/3"), which docs keep.

use crate::modules::common::err;
use crate::modules::notes;
use rusqlite::Connection;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Db {
    Pomelo,
    Picky,
}

pub struct Source {
    pub module: &'static str,
    pub kind: &'static str,
    pub label: &'static str,
    pub db: Db,
    sql: &'static str,
}

impl Source {
    /// Rows of several made modules (key from the 8th column).
    pub fn dynamic(&self) -> bool {
        self.module.ends_with('/')
    }
    /// A doc of `module` comes from this source.
    pub fn owns(&self, module: &str) -> bool {
        if self.dynamic() {
            module.starts_with(self.module)
        } else {
            self.module == module
        }
    }
}

pub const SOURCES: &[Source] = &[
    Source { module: "todo", kind: "group", label: "待办分组", db: Db::Pomelo, sql: "SELECT id, name, NULL, NULL, NULL, created_at, name FROM todo_groups" },
    Source {
        module: "todo",
        kind: "goal",
        label: "目标",
        db: Db::Pomelo,
        sql: "SELECT g.id, g.name, gr.name, g.group_id, g.end_date, g.created_at, concat_ws(char(10), g.name, g.note, g.review)
              FROM todo_goals g JOIN todo_groups gr ON gr.id = g.group_id",
    },
    Source {
        module: "todo",
        kind: "task",
        label: "任务",
        db: Db::Pomelo,
        // parent = -goal id when in a goal (the page opens that goal), else the group id
        sql: "SELECT t.id, t.title, coalesce(go.name, gr.name) || CASE WHEN t.done THEN ' · 已完成' ELSE '' END,
                CASE WHEN t.goal_id IS NULL THEN t.group_id ELSE -t.goal_id END, t.due_date, t.created_at, concat_ws(char(10), t.title, t.note)
              FROM todo_tasks t JOIN todo_groups gr ON gr.id = t.group_id LEFT JOIN todo_goals go ON go.id = t.goal_id",
    },
    Source {
        module: "books",
        kind: "book",
        label: "书",
        db: Db::Pomelo,
        sql: "SELECT b.id, b.title, b.author, NULL, NULL, b.created_at, concat_ws(char(10), b.title, b.author, b.country, k.name, b.note)
              FROM books b LEFT JOIN kinds k ON k.id = b.kind_id",
    },
    Source {
        module: "films",
        kind: "film",
        label: "影视",
        db: Db::Pomelo,
        sql: "SELECT f.id, f.title, f.director, NULL, NULL, f.created_at, concat_ws(char(10), f.title, f.director, f.country, k.name, f.note)
              FROM films f LEFT JOIN kinds k ON k.id = f.kind_id",
    },
    Source { module: "drawing", kind: "course", label: "绘画课程", db: Db::Pomelo, sql: "SELECT id, name, NULL, NULL, NULL, created_at, concat_ws(char(10), name, note) FROM drawing_courses" },
    Source {
        module: "drawing",
        kind: "lesson",
        label: "绘画课节",
        db: Db::Pomelo,
        sql: "SELECT l.id, coalesce(l.code || ' ', '') || l.title, c.name, l.course_id, NULL, l.created_at,
                concat_ws(char(10), coalesce(l.code || ' ', '') || l.title, k.name, l.note)
              FROM drawing_lessons l JOIN drawing_courses c ON c.id = l.course_id LEFT JOIN kinds k ON k.id = l.kind_id",
    },
    Source { module: "packing", kind: "trip", label: "出行", db: Db::Pomelo, sql: "SELECT id, name, NULL, NULL, depart_on, created_at, concat_ws(char(10), name, note) FROM trips" },
    Source { module: "packing", kind: "template", label: "打包模板", db: Db::Pomelo, sql: "SELECT id, name, '模板', NULL, NULL, created_at, concat_ws(char(10), name, note) FROM packing_templates" },
    Source {
        module: "packing",
        kind: "trip_item",
        label: "出行物品",
        db: Db::Pomelo,
        sql: "SELECT i.id, i.name, t.name, i.trip_id, NULL, i.created_at, concat_ws(char(10), i.name, k.name, i.note)
              FROM trip_items i JOIN trips t ON t.id = i.trip_id LEFT JOIN kinds k ON k.id = i.kind_id",
    },
    Source {
        module: "packing",
        kind: "template_item",
        label: "模板物品",
        db: Db::Pomelo,
        sql: "SELECT i.id, i.name, '模板 · ' || t.name, i.template_id, NULL, i.created_at, concat_ws(char(10), i.name, k.name, i.note)
              FROM packing_template_items i JOIN packing_templates t ON t.id = i.template_id LEFT JOIN kinds k ON k.id = i.kind_id",
    },
    Source {
        module: "finance",
        kind: "entry",
        label: "收支",
        db: Db::Pomelo,
        sql: "SELECT e.id, coalesce(e.note, k.name, '未分类'), k.name, NULL, e.day, e.created_at, concat_ws(char(10), e.note, k.name)
              FROM fin_entries e LEFT JOIN kinds k ON k.id = e.kind_id",
    },
    Source {
        module: "finance",
        kind: "fixed",
        label: "固定支出",
        db: Db::Pomelo,
        sql: "SELECT f.id, f.name, '固定支出', NULL, NULL, f.created_at, concat_ws(char(10), f.name, k.name, f.note) FROM fin_fixed f LEFT JOIN kinds k ON k.id = f.kind_id",
    },
    Source { module: "finance", kind: "account", label: "账户", db: Db::Pomelo, sql: "SELECT id, name, '账户', NULL, NULL, created_at, concat_ws(char(10), name, note) FROM fin_accounts" },
    Source {
        module: "jobs",
        kind: "job",
        label: "投递",
        db: Db::Pomelo,
        sql: "SELECT j.id, j.company || coalesce(' · ' || j.position, ''), j.location, NULL, NULL, j.created_at,
                concat_ws(char(10), j.company, j.position, j.location, k.name, j.note)
              FROM jobs j LEFT JOIN kinds k ON k.id = j.kind_id",
    },
    Source { module: "notes", kind: "note", label: "笔记", db: Db::Pomelo, sql: "SELECT id, content, NULL, NULL, date(created_at, 'localtime'), created_at, content FROM notes" },
    Source { module: "picky", kind: "session", label: "小拾对话", db: Db::Picky, sql: "SELECT id, title, NULL, NULL, NULL, created_at, title FROM sessions" },
    Source {
        module: "picky",
        kind: "message",
        label: "小拾对话",
        db: Db::Picky,
        // parent = the conversation; detail = who wrote it; reasoning never
        sql: "SELECT m.id, s.title, CASE m.role WHEN 'user' THEN '你' ELSE coalesce(r.model_name, '小拾') END, m.session_id, NULL, m.created_at, m.content
              FROM messages m JOIN sessions s ON s.id = m.session_id LEFT JOIN runs r ON r.id = m.run_id
              WHERE m.state <> 'streaming' AND trim(m.content) <> ''",
    },
    // last: a source's place numbers its vectors. 证件: the name only (never what a note might hold)
    Source {
        module: "days",
        kind: "day",
        label: "日子",
        db: Db::Pomelo,
        sql: "SELECT id, name, CASE kind WHEN 'birthday' THEN '生日' WHEN 'document' THEN '证件' ELSE '纪念日' END, NULL, NULL, created_at,
                CASE kind WHEN 'document' THEN name ELSE concat_ws(char(10), name, note, ideas) END FROM days",
    },
    // 五花八门: 名称 and the words in its 文字 / 长文字 / 单选 / 多选 / 链接 fields
    Source {
        module: "list/",
        kind: "list_item",
        label: "清单条目",
        db: Db::Pomelo,
        sql: "SELECT i.id, i.name, m.name, NULL, NULL, i.created_at,
                concat_ws(char(10), i.name, (SELECT group_concat(CASE f.type WHEN 'multi' THEN (SELECT group_concat(x.value, ' ') FROM json_each(j.value) x) ELSE j.value END, char(10))
                  FROM json_each(i.data) j JOIN list_fields f ON f.id = CAST(j.key AS INTEGER) WHERE f.type IN ('text', 'longtext', 'select', 'multi', 'link'))),
                'list/' || i.module_id
              FROM list_items i JOIN custom_modules m ON m.id = i.module_id",
    },
    // 循序渐进
    Source {
        module: "progress/",
        kind: "progress_course",
        label: "进度",
        db: Db::Pomelo,
        sql: "SELECT c.id, c.name, m.name, NULL, NULL, c.created_at, concat_ws(char(10), c.name, c.note), 'progress/' || c.module_id FROM progress_courses c JOIN custom_modules m ON m.id = c.module_id",
    },
    Source {
        module: "progress/",
        kind: "progress_lesson",
        label: "进度里的一节",
        db: Db::Pomelo,
        sql: "SELECT l.id, coalesce(l.code || ' ', '') || l.title, c.name, l.course_id, NULL, l.created_at, concat_ws(char(10), coalesce(l.code || ' ', '') || l.title, k.name, l.note),
                'progress/' || c.module_id
              FROM progress_lessons l JOIN progress_courses c ON c.id = l.course_id LEFT JOIN kinds k ON k.id = l.kind_id",
    },
];

pub fn source(module: &str, kind: &str) -> Option<(usize, &'static Source)> {
    SOURCES.iter().enumerate().find(|(_, s)| s.kind == kind && s.owns(module))
}

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub ref_id: i64,
    pub title: String,
    pub detail: Option<String>,
    pub parent: Option<i64>,
    pub day: Option<String>,
    pub at: String,
    pub body: String,
    /// Dynamic sources: the row's module key.
    pub module: Option<String>,
}

impl Row {
    /// Everything stored: a change rewrites the doc.
    pub fn hash(&self) -> i64 {
        let parent = self.parent.map(|p| p.to_string()).unwrap_or_default();
        super::text::hash(&[&self.title, self.detail.as_deref().unwrap_or(""), &parent, self.day.as_deref().unwrap_or(""), &self.at, &self.body])
    }
}

/// What the vectors depend on: a change makes them stale.
pub fn vector_hash(s: &Source, body: &str) -> i64 {
    super::text::hash(&[s.label, body, &super::text::RULES.to_string()])
}

/// Markdown bold marks out, lines trimmed, empty lines dropped.
fn clean(s: &str) -> String {
    s.replace("**", "").lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n")
}

pub fn read(conn: &Connection, s: &Source) -> Result<Vec<Row>, String> {
    let mut st = conn.prepare(s.sql).map_err(err)?;
    let rows = st
        .query_map([], |r| {
            Ok(Row {
                ref_id: r.get(0)?,
                title: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                detail: r.get(2)?,
                parent: r.get(3)?,
                day: r.get(4)?,
                at: r.get::<_, Option<String>>(5)?.unwrap_or_default(),
                body: r.get::<_, Option<String>>(6)?.unwrap_or_default(),
                module: if s.dynamic() { Some(r.get(7)?) } else { None },
            })
        })
        .map_err(err)?;
    let mut out = vec![];
    for row in rows {
        let mut row = row.map_err(err)?;
        row.title = if s.kind == "note" { notes::excerpt(&row.title) } else { clean(&row.title).replace('\n', " ") };
        row.body = clean(&row.body);
        out.push(row);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_source_reads_and_the_vault_is_not_one() {
        let c = crate::db::tests::memory_db();
        let p = crate::picky::store::tests::memory_db();
        c.execute_batch(
            "INSERT INTO todo_groups (name, sort_order) VALUES ('学业', 1);
             INSERT INTO todo_tasks (group_id, title, note, sort_order) VALUES (1, '整理**时间**表', '  第一行\n\n  第二行 ', 1);
             INSERT INTO books (title, author, note) VALUES ('小王子', '圣埃克苏佩里', NULL);
             INSERT INTO notes (content, created_at) VALUES ('#读书 关于**时间**的想法', '2026-10-01T01:00:00.000Z');
             INSERT INTO days (kind, name, date, ideas) VALUES ('birthday', '妈妈', '1966-11-02', '围巾'), ('document', '护照', '2030-01-01', NULL);
             UPDATE days SET note = 'E1234567' WHERE kind = 'document';
             INSERT INTO custom_modules (template, name, icon) VALUES ('list', '游戏', 'shapes'), ('progress', '动画', 'tv');
             INSERT INTO list_fields (module_id, name, type, options, sort_order) VALUES (1, '平台', 'select', '[\"Switch\"]', 1), (1, '标签', 'multi', '[\"解谜\",\"冒险\"]', 2), (1, '评分', 'rating', '[]', 3);
             INSERT INTO list_items (module_id, name, data, sort_order) VALUES (1, '塞尔达', '{\"1\":\"Switch\",\"2\":[\"解谜\",\"冒险\"],\"3\":5}', 1);
             INSERT INTO progress_courses (module_id, name, sort_order) VALUES (2, '第一季', 1);
             INSERT INTO progress_lessons (course_id, code, title, sort_order) VALUES (1, '1', '出发', 1);",
        )
        .unwrap();
        p.execute_batch(
            "INSERT INTO sessions (title) VALUES ('向量数据库');
             INSERT INTO messages (session_id, role, content, state) VALUES (1, 'user', '怎么选？', 'complete'), (1, 'assistant', '', 'complete'), (1, 'assistant', '写到一半', 'streaming');",
        )
        .unwrap();
        for s in SOURCES {
            let rows = read(if s.db == Db::Pomelo { &c } else { &p }, s).unwrap();
            match s.kind {
                "task" => {
                    assert_eq!((rows[0].title.as_str(), rows[0].body.as_str()), ("整理时间表", "整理时间表\n第一行\n第二行"));
                    assert_eq!((rows[0].parent, rows[0].detail.as_deref()), (Some(1), Some("学业")));
                }
                "book" => assert_eq!(rows[0].body, "小王子\n圣埃克苏佩里"),
                "note" => assert_eq!((rows[0].title.as_str(), rows[0].day.as_deref()), ("关于时间的想法", Some("2026-10-01"))),
                "message" => assert_eq!((rows.len(), rows[0].detail.as_deref(), rows[0].parent), (1, Some("你"), Some(1))), // empty and streaming left out
                "list_item" => assert_eq!((rows[0].body.as_str(), rows[0].detail.as_deref(), rows[0].module.as_deref()), ("塞尔达\nSwitch\n解谜 冒险", Some("游戏"), Some("list/1"))),
                "progress_lesson" => assert_eq!((rows[0].title.as_str(), rows[0].parent, rows[0].module.as_deref()), ("1 出发", Some(1), Some("progress/2"))),
                "day" => assert_eq!(rows.iter().map(|r| (r.title.as_str(), r.detail.as_deref().unwrap(), r.body.as_str())).collect::<Vec<_>>(), [("妈妈", "生日", "妈妈\n围巾"), ("护照", "证件", "护照")]),
                _ => {}
            }
        }
        assert!(SOURCES.iter().all(|s| s.module != "vault" && !s.sql.contains("vault")));
        assert_eq!(source("notes", "note").unwrap().0, 15);
        assert!(SOURCES.len() <= 32); // vector filters use one bit per source
    }
}
