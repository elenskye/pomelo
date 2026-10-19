//! 循序渐进 (v0.4.5): like 随意画画 — courses → lessons, called what the module says (custom.rs
//! `course_word` / `lesson_word`, default 课程 / 课节). Rules (all here):
//! - 类型: each module its own kinds list (kinds.rs, module "progress/<id>", none to start); a lesson
//!   has one or none (未分类; deleting a kind leaves its lessons 未分类).
//! - Course names: 1–100 chars, unique in the module. Lessons: 编号 (display only, 20), 名称 (200),
//!   status todo 未开始 / doing 进行中 / done 已完成, 完成月份 (filled with this month when marked
//!   done without one), 备注 (5000). New ones go last; both lists are dragged into order.
//! - Pace and 下一节 as 随意画画 (drawing::pace_of): finished in the last 6 months → months left.

use super::common::{self, err, some, Status};
use super::custom::{self, Template};
use super::{drawing, kinds};
use crate::AppState;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use tauri::State;

const NOW: &str = "strftime('%Y-%m-%dT%H:%M:%fZ', 'now')";

// ---------- courses ----------

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Course {
    pub id: i64,
    pub module_id: i64,
    pub name: String,
    pub note: Option<String>,
    pub sort_order: i64,
    pub total: i64,
    pub done: i64,
    pub doing: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CoursePatch {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "some")]
    pub note: Option<Option<String>>,
}

const COURSE_SQL: &str = "SELECT c.id, c.module_id, c.name, c.note, c.sort_order,
    (SELECT count(*) FROM progress_lessons l WHERE l.course_id = c.id),
    (SELECT count(*) FROM progress_lessons l WHERE l.course_id = c.id AND l.status = 'done'),
    (SELECT count(*) FROM progress_lessons l WHERE l.course_id = c.id AND l.status = 'doing'),
    c.created_at, c.updated_at
  FROM progress_courses c";

fn course_row(r: &Row) -> rusqlite::Result<Course> {
    Ok(Course {
        id: r.get(0)?,
        module_id: r.get(1)?,
        name: r.get(2)?,
        note: r.get(3)?,
        sort_order: r.get(4)?,
        total: r.get(5)?,
        done: r.get(6)?,
        doing: r.get(7)?,
        created_at: r.get(8)?,
        updated_at: r.get(9)?,
    })
}

pub fn courses(conn: &Connection, module_id: i64) -> Result<Vec<Course>, String> {
    let mut st = conn.prepare(&format!("{COURSE_SQL} WHERE c.module_id = ?1 ORDER BY c.sort_order, c.id")).map_err(err)?;
    let rows = st.query_map([module_id], course_row).map_err(err)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(err)
}

/// The words of the module a course is in (for messages).
fn words(conn: &Connection, module_id: i64) -> (String, String) {
    custom::get(conn, module_id).map(|m| (m.course_word, m.lesson_word)).unwrap_or_else(|_| (custom::DEFAULT_WORDS.0.into(), custom::DEFAULT_WORDS.1.into()))
}

pub fn course(conn: &Connection, id: i64) -> Result<Course, String> {
    conn.query_row(&format!("{COURSE_SQL} WHERE c.id = ?1"), [id], course_row).optional().map_err(err)?.ok_or_else(|| "找不到这一项，可能已经被删除了".to_string())
}

fn clean_course(conn: &Connection, module_id: i64, name: &str, note: Option<String>, except: Option<i64>) -> Result<(String, Option<String>), String> {
    let word = words(conn, module_id).0;
    let name = common::require(name, &format!("{word}名"), 100)?;
    let note = common::clean(note);
    common::check_len(&note, "备注", 5000)?;
    let dup: i64 = conn.query_row("SELECT count(*) FROM progress_courses WHERE module_id = ?1 AND name = ?2 AND id != coalesce(?3, -1)", params![module_id, name, except], |r| r.get(0)).map_err(err)?;
    if dup > 0 {
        return Err(format!("已经有叫「{name}」的{word}了"));
    }
    Ok((name, note))
}

pub fn create_course(conn: &Connection, module_id: i64, p: CoursePatch) -> Result<Course, String> {
    custom::of(conn, Template::Progress, module_id)?;
    let (name, note) = clean_course(conn, module_id, p.name.as_deref().unwrap_or(""), p.note.flatten(), None)?;
    conn.execute(
        "INSERT INTO progress_courses (module_id, name, note, sort_order) VALUES (?1, ?2, ?3, (SELECT coalesce(max(sort_order), 0) + 1 FROM progress_courses WHERE module_id = ?1))",
        params![module_id, name, note],
    )
    .map_err(err)?;
    course(conn, conn.last_insert_rowid())
}

pub fn update_course(conn: &Connection, id: i64, p: CoursePatch) -> Result<Course, String> {
    let old = course(conn, id)?;
    let (name, note) = clean_course(conn, old.module_id, p.name.as_deref().unwrap_or(&old.name), p.note.unwrap_or(old.note), Some(id))?;
    conn.execute(&format!("UPDATE progress_courses SET name = ?1, note = ?2, updated_at = {NOW} WHERE id = ?3"), params![name, note, id]).map_err(err)?;
    course(conn, id)
}

/// Deletes the course and its lessons (FK cascade).
pub fn delete_course(conn: &Connection, id: i64) -> Result<(), String> {
    match conn.execute("DELETE FROM progress_courses WHERE id = ?1", [id]).map_err(err)? {
        0 => Err("找不到这一项，可能已经被删除了".into()),
        _ => Ok(()),
    }
}

// ---------- lessons ----------

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Lesson {
    pub id: i64,
    pub course_id: i64,
    pub code: Option<String>,
    pub kind_id: Option<i64>,
    pub title: String,
    pub status: Status,
    pub finished_month: Option<String>,
    pub note: Option<String>,
    pub sort_order: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LessonPatch {
    #[serde(default, deserialize_with = "some")]
    pub code: Option<Option<String>>,
    #[serde(default, deserialize_with = "some")]
    pub kind_id: Option<Option<i64>>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub status: Option<Status>,
    #[serde(default, deserialize_with = "some")]
    pub finished_month: Option<Option<String>>,
    #[serde(default, deserialize_with = "some")]
    pub note: Option<Option<String>>,
}

#[derive(Debug, Clone, PartialEq)]
struct Fields {
    code: Option<String>,
    kind_id: Option<i64>,
    title: String,
    status: Status,
    finished_month: Option<String>,
    note: Option<String>,
}

impl Fields {
    fn apply(self, p: LessonPatch) -> Fields {
        Fields {
            code: p.code.unwrap_or(self.code),
            kind_id: p.kind_id.unwrap_or(self.kind_id),
            title: p.title.unwrap_or(self.title),
            status: p.status.unwrap_or(self.status),
            finished_month: p.finished_month.unwrap_or(self.finished_month),
            note: p.note.unwrap_or(self.note),
        }
    }
    fn validated(self) -> Result<Fields, String> {
        let f = Fields {
            code: common::clean(self.code),
            kind_id: self.kind_id,
            title: common::require(&self.title, "名称", 200)?,
            // 待标记 isn't a state here
            status: if self.status == Status::Unmarked { Status::Todo } else { self.status },
            finished_month: common::clean(self.finished_month),
            note: common::clean(self.note),
        };
        common::check_len(&f.code, "编号", 20)?;
        common::check_len(&f.note, "备注", 5000)?;
        common::check_month(&f.finished_month)?;
        Ok(f)
    }
}

const LESSON_COLS: &str = "id, course_id, code, title, status, finished_month, note, sort_order, created_at, updated_at, kind_id";

fn lesson_row(r: &Row) -> rusqlite::Result<Lesson> {
    Ok(Lesson {
        id: r.get(0)?,
        course_id: r.get(1)?,
        code: r.get(2)?,
        title: r.get(3)?,
        status: r.get(4)?,
        finished_month: r.get(5)?,
        note: r.get(6)?,
        sort_order: r.get(7)?,
        created_at: r.get(8)?,
        updated_at: r.get(9)?,
        kind_id: r.get(10)?,
    })
}

pub fn lessons(conn: &Connection, course_id: i64) -> Result<Vec<Lesson>, String> {
    let mut st = conn.prepare(&format!("SELECT {LESSON_COLS} FROM progress_lessons WHERE course_id = ?1 ORDER BY sort_order, id")).map_err(err)?;
    let rows = st.query_map([course_id], lesson_row).map_err(err)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(err)
}

pub fn lesson(conn: &Connection, id: i64) -> Result<Lesson, String> {
    conn.query_row(&format!("SELECT {LESSON_COLS} FROM progress_lessons WHERE id = ?1"), [id], lesson_row).optional().map_err(err)?.ok_or_else(|| "找不到这一节，可能已经被删除了".to_string())
}

/// 随意画画's pace over these lessons.
pub fn pace(lessons: &[Lesson], today: &str) -> drawing::Pace {
    drawing::pace_of(lessons.iter().map(|l| (l.status, l.finished_month.as_deref())), today)
}

/// The one to do next: the first 进行中, else the first not done.
pub fn next(lessons: &[Lesson]) -> Option<&Lesson> {
    lessons.iter().find(|l| l.status == Status::Doing).or_else(|| lessons.iter().find(|l| l.status != Status::Done))
}

/// The kinds list a course's lessons use.
fn kinds_key(conn: &Connection, course_id: i64) -> Result<String, String> {
    Ok(custom::key(Template::Progress, course(conn, course_id)?.module_id))
}

pub fn create_lesson(conn: &Connection, course_id: i64, p: LessonPatch) -> Result<Lesson, String> {
    let key = kinds_key(conn, course_id)?;
    let given = p.finished_month.is_some();
    let mut f = Fields { code: None, kind_id: None, title: String::new(), status: Status::Todo, finished_month: None, note: None }.apply(p).validated()?;
    kinds::check(conn, &key, f.kind_id)?;
    common::auto_finished(conn, f.status, false, &mut f.finished_month, given, 7)?;
    conn.execute(
        "INSERT INTO progress_lessons (course_id, code, kind_id, title, status, finished_month, note, sort_order)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, (SELECT coalesce(max(sort_order), 0) + 1 FROM progress_lessons WHERE course_id = ?1))",
        params![course_id, f.code, f.kind_id, f.title, f.status, f.finished_month, f.note],
    )
    .map_err(err)?;
    lesson(conn, conn.last_insert_rowid())
}

pub fn update_lesson(conn: &Connection, id: i64, p: LessonPatch) -> Result<Lesson, String> {
    let old = lesson(conn, id)?;
    let given = p.finished_month.is_some();
    let was = Fields { code: old.code.clone(), kind_id: old.kind_id, title: old.title.clone(), status: old.status, finished_month: old.finished_month.clone(), note: old.note.clone() };
    let mut f = was.apply(p).validated()?;
    kinds::check(conn, &kinds_key(conn, old.course_id)?, f.kind_id)?;
    common::auto_finished(conn, f.status, old.status == Status::Done, &mut f.finished_month, given, 7)?;
    conn.execute(
        &format!("UPDATE progress_lessons SET code = ?1, kind_id = ?2, title = ?3, status = ?4, finished_month = ?5, note = ?6, updated_at = {NOW} WHERE id = ?7"),
        params![f.code, f.kind_id, f.title, f.status, f.finished_month, f.note, id],
    )
    .map_err(err)?;
    lesson(conn, id)
}

pub fn delete_lesson(conn: &Connection, id: i64) -> Result<(), String> {
    match conn.execute("DELETE FROM progress_lessons WHERE id = ?1", [id]).map_err(err)? {
        0 => Err("找不到这一节，可能已经被删除了".into()),
        _ => Ok(()),
    }
}

// ---------- export / import ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LessonRecord {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// 类型 by name (files before v18 have none)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    pub title: String,
    #[serde(default = "todo")]
    pub status: Status,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_month: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

fn todo() -> Status {
    Status::Todo
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CourseRecord {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default)]
    pub lessons: Vec<LessonRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

impl LessonRecord {
    fn fields(&self) -> Result<Fields, String> {
        kinds::check_name(&self.kind)?;
        Fields { code: self.code.clone(), kind_id: None, title: self.title.clone(), status: self.status, finished_month: self.finished_month.clone(), note: self.note.clone() }.validated()
    }
}

pub fn validate_records(rec: &[CourseRecord]) -> Result<(), String> {
    let mut names = std::collections::HashSet::new();
    for (i, c) in rec.iter().enumerate() {
        let name = common::require(&c.name, "名称", 100).map_err(|e| format!("第 {} 项：{e}", i + 1))?;
        if !names.insert(name.clone()) {
            return Err(format!("「{name}」重复了"));
        }
        common::check_len(&common::clean(c.note.clone()), "备注", 5000)?;
        for (j, l) in c.lessons.iter().enumerate() {
            l.fields().map_err(|e| format!("「{name}」第 {} 节（{}）：{e}", j + 1, l.title.trim()))?;
        }
    }
    Ok(())
}

/// Writes a module's courses and lessons (already checked); kinds named by lessons but missing from
/// the module's list are added. Caller owns the transaction.
pub fn import(conn: &Connection, module_id: i64, rec: &[CourseRecord]) -> Result<(), String> {
    let key = custom::key(Template::Progress, module_id);
    for (i, c) in rec.iter().enumerate() {
        conn.execute(
            &format!("INSERT INTO progress_courses (module_id, name, note, sort_order, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, coalesce(?5, {NOW}), coalesce(?6, ?5, {NOW}))"),
            params![module_id, c.name.trim(), common::clean(c.note.clone()), i as i64 + 1, c.created_at, c.updated_at],
        )
        .map_err(err)?;
        let course_id = conn.last_insert_rowid();
        for (j, l) in c.lessons.iter().enumerate() {
            let f = l.fields()?;
            let kind_id = kinds::resolve(conn, &key, l.kind.as_deref())?;
            conn.execute(
                &format!(
                    "INSERT INTO progress_lessons (course_id, code, kind_id, title, status, finished_month, note, sort_order, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, coalesce(?9, {NOW}), coalesce(?10, ?9, {NOW}))"
                ),
                params![course_id, f.code, kind_id, f.title, f.status, f.finished_month, f.note, j as i64 + 1, l.created_at, l.updated_at],
            )
            .map_err(err)?;
        }
    }
    Ok(())
}

pub fn export(conn: &Connection, module_id: i64) -> Result<Vec<CourseRecord>, String> {
    let names = kinds::name_map(conn, &custom::key(Template::Progress, module_id))?;
    courses(conn, module_id)?
        .into_iter()
        .map(|c| {
            let lessons = lessons(conn, c.id)?
                .into_iter()
                .map(|l| LessonRecord { code: l.code, kind: l.kind_id.and_then(|k| names.get(&k).cloned()), title: l.title, status: l.status, finished_month: l.finished_month, note: l.note, created_at: Some(l.created_at), updated_at: Some(l.updated_at) })
                .collect();
            Ok(CourseRecord { name: c.name, note: c.note, lessons, created_at: Some(c.created_at), updated_at: Some(c.updated_at) })
        })
        .collect()
}

// ---------- commands (undo labels in the module's own words) ----------

fn word_of_course(state: &State<AppState>, course_id: i64) -> (String, String) {
    state.with_db(|c| Ok(course(c, course_id).map(|x| words(c, x.module_id)).unwrap_or_else(|_| words(c, 0)))).unwrap_or_default()
}

fn word_of_lesson(state: &State<AppState>, id: i64) -> String {
    state.with_db(|c| Ok(lesson(c, id).and_then(|l| course(c, l.course_id)).map(|x| words(c, x.module_id).1).unwrap_or_else(|_| words(c, 0).1))).unwrap_or_default()
}

#[tauri::command]
pub fn progress_courses(state: State<AppState>, module_id: i64) -> Result<Vec<Course>, String> {
    state.with_db(|c| courses(c, module_id))
}

#[tauri::command]
pub fn progress_course_create(state: State<AppState>, module_id: i64, patch: CoursePatch) -> Result<Course, String> {
    let w = state.with_db(|c| Ok(words(c, module_id).0))?;
    state.with_undo(&format!("添加{w}"), |c| create_course(c, module_id, patch))
}

#[tauri::command]
pub fn progress_course_update(state: State<AppState>, id: i64, patch: CoursePatch) -> Result<Course, String> {
    let w = word_of_course(&state, id).0;
    state.with_undo(&format!("修改{w}"), |c| update_course(c, id, patch))
}

#[tauri::command]
pub fn progress_course_delete(state: State<AppState>, id: i64) -> Result<(), String> {
    let w = word_of_course(&state, id).0;
    state.with_undo(&format!("删除{w}"), |c| delete_course(c, id))
}

#[tauri::command]
pub fn progress_courses_reorder(state: State<AppState>, module_id: i64, ids: Vec<i64>) -> Result<(), String> {
    state.with_undo("调整顺序", |c| common::reorder(c, "progress_courses", "module_id = ?1", &[&module_id], &ids))
}

#[tauri::command]
pub fn progress_lessons(state: State<AppState>, course_id: i64) -> Result<Vec<Lesson>, String> {
    state.with_db(|c| lessons(c, course_id))
}

#[tauri::command]
pub fn progress_lesson_create(state: State<AppState>, course_id: i64, patch: LessonPatch) -> Result<Lesson, String> {
    let w = word_of_course(&state, course_id).1;
    state.with_undo(&format!("添加{w}"), |c| create_lesson(c, course_id, patch))
}

#[tauri::command]
pub fn progress_lesson_update(state: State<AppState>, id: i64, patch: LessonPatch) -> Result<Lesson, String> {
    let w = word_of_lesson(&state, id);
    state.with_undo(&format!("修改{w}"), |c| update_lesson(c, id, patch))
}

#[tauri::command]
pub fn progress_lesson_delete(state: State<AppState>, id: i64) -> Result<(), String> {
    let w = word_of_lesson(&state, id);
    state.with_undo(&format!("删除{w}"), |c| delete_lesson(c, id))
}

#[tauri::command]
pub fn progress_lessons_reorder(state: State<AppState>, course_id: i64, ids: Vec<i64>) -> Result<(), String> {
    state.with_undo("调整顺序", |c| common::reorder(c, "progress_lessons", "course_id = ?1", &[&course_id], &ids))
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::db::tests::memory_db;
    use serde_json::json;

    fn from<T: serde::de::DeserializeOwned>(v: serde_json::Value) -> T {
        serde_json::from_value(v).unwrap()
    }

    /// 动画 (季 / 集, 类型 正篇 · 番外): 第一季 with 3 episodes (1 done in 2026-09, 1 doing; the first 正篇).
    pub fn seed(c: &Connection) -> (i64, i64, [i64; 3]) {
        let m = custom::create(c, from(json!({ "template": "progress", "name": "动画", "icon": "tv", "courseWord": "季", "lessonWord": "集" }))).unwrap().id;
        let key = custom::key(Template::Progress, m);
        let main = kinds::create(c, &key, "正篇").unwrap().id;
        kinds::create(c, &key, "番外").unwrap();
        let s = create_course(c, m, from(json!({ "name": "第一季" }))).unwrap().id;
        let l = |v: serde_json::Value| create_lesson(c, s, from(v)).unwrap().id;
        (m, s, [l(json!({ "code": "1", "kindId": main, "title": "出发", "status": "done", "finishedMonth": "2026-09" })), l(json!({ "code": "2", "title": "相遇", "status": "doing" })), l(json!({ "code": "3", "title": "告别" }))])
    }

    #[test]
    fn each_module_its_own_kinds() {
        let c = memory_db();
        let (m, _, [a, b, _]) = seed(&c);
        let key = custom::key(Template::Progress, m);
        let names: Vec<String> = kinds::list(&c, &key).unwrap().into_iter().map(|k| k.name).collect();
        assert_eq!(names, ["正篇", "番外"]);
        assert_eq!(kinds::list(&c, &key).unwrap()[0].count, 1);
        let extra = kinds::list(&c, &key).unwrap()[1].id;
        assert_eq!(update_lesson(&c, b, from(json!({ "kindId": extra }))).unwrap().kind_id, Some(extra));
        // another module's kind (or 随意画画's) doesn't fit
        let other = custom::create(&c, from(json!({ "template": "progress", "name": "日语", "icon": "languages" }))).unwrap().id;
        let theirs = kinds::create(&c, &custom::key(Template::Progress, other), "语法").unwrap().id;
        assert!(update_lesson(&c, a, from(json!({ "kindId": theirs }))).is_err());
        assert!(kinds::create(&c, "progress/999", "x").is_err());
        // deleting a kind leaves 未分类; deleting the module takes its list
        kinds::delete(&c, extra).unwrap();
        assert_eq!(lesson(&c, b).unwrap().kind_id, None);
        custom::delete(&c, m).unwrap();
        assert!(kinds::list(&c, &key).unwrap().is_empty());
        assert_eq!(kinds::list(&c, &custom::key(Template::Progress, other)).unwrap().len(), 1);
    }

    #[test]
    fn courses_lessons_pace_and_words() {
        let c = memory_db();
        let (m, s, [a, b, _]) = seed(&c);
        let course = &courses(&c, m).unwrap()[0];
        assert_eq!((course.total, course.done, course.doing), (3, 1, 1));
        assert!(create_course(&c, m, from(json!({ "name": "第一季" }))).unwrap_err().contains("已经有叫「第一季」的季了"));
        let ls = lessons(&c, s).unwrap();
        assert_eq!(next(&ls).unwrap().id, b);
        let p = pace(&ls, "2026-10-17");
        assert_eq!((p.recent, p.remaining, p.months_left), (1, 2, Some(12)));
        // done without a month: this month; 待标记 is 未开始 here
        let x = update_lesson(&c, b, from(json!({ "status": "done" }))).unwrap();
        assert!(x.finished_month.is_some());
        assert_eq!(update_lesson(&c, a, from(json!({ "status": "unmarked" }))).unwrap().status, Status::Todo);
        // a list module can't hold courses; deleting the module takes everything
        let list = custom::create(&c, from(json!({ "template": "list", "name": "清单", "icon": "shapes" }))).unwrap().id;
        assert!(create_course(&c, list, from(json!({ "name": "x" }))).is_err());
        custom::delete(&c, m).unwrap();
        assert!(lesson(&c, a).is_err());
    }

    #[test]
    fn export_import_roundtrip() {
        let a = memory_db();
        let (m, _, _) = seed(&a);
        let rec = export(&a, m).unwrap();
        validate_records(&rec).unwrap();
        let b = memory_db();
        let m2 = custom::create(&b, from(json!({ "template": "progress", "name": "动画", "icon": "tv" }))).unwrap().id;
        import(&b, m2, &rec).unwrap();
        assert_eq!(serde_json::to_value(export(&b, m2).unwrap()).unwrap(), serde_json::to_value(&rec).unwrap());
    }
}
