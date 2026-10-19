//! 随意画画 (drawing courses → lessons, progress). 类型 = kinds.rs (module "drawing").
//! New lessons append to the end of their course; order comes from import / creation.
//! Status: 未开始 / 进行中 / 已完成 (no 待标记: one given — an old data file — becomes 未开始).
//! Pace (小拾 「照现在的速度什么时候画完」): lessons finished in the last `PACE_MONTHS` months (this
//! month included) ÷ `PACE_MONTHS` = per month; remaining ÷ that, rounded up = months left.

use super::common::{self, err, some, Status};
use super::kinds;
use crate::AppState;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use tauri::State;

pub const KEY: &str = "drawing";
pub const LABEL: &str = "随意画画";
const COURSE_NOT_FOUND: &str = "找不到这门课程，可能已经被删除了";
const LESSON_NOT_FOUND: &str = "找不到这节课，可能已经被删除了";
const NOW: &str = "strftime('%Y-%m-%dT%H:%M:%fZ', 'now')";

// ---------- courses ----------

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Course {
    pub id: i64,
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

const COURSE_SQL: &str = "SELECT c.id, c.name, c.note, c.sort_order,
    (SELECT count(*) FROM drawing_lessons l WHERE l.course_id = c.id),
    (SELECT count(*) FROM drawing_lessons l WHERE l.course_id = c.id AND l.status = 'done'),
    (SELECT count(*) FROM drawing_lessons l WHERE l.course_id = c.id AND l.status = 'doing'),
    c.created_at, c.updated_at
  FROM drawing_courses c";

fn course_row(r: &Row) -> rusqlite::Result<Course> {
    Ok(Course {
        id: r.get(0)?,
        name: r.get(1)?,
        note: r.get(2)?,
        sort_order: r.get(3)?,
        total: r.get(4)?,
        done: r.get(5)?,
        doing: r.get(6)?,
        created_at: r.get(7)?,
        updated_at: r.get(8)?,
    })
}

pub fn courses(conn: &Connection) -> Result<Vec<Course>, String> {
    let mut st = conn.prepare(&format!("{COURSE_SQL} ORDER BY c.sort_order, c.id")).map_err(err)?;
    let rows = st.query_map([], course_row).map_err(err)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(err)
}

pub fn course(conn: &Connection, id: i64) -> Result<Course, String> {
    conn.query_row(&format!("{COURSE_SQL} WHERE c.id = ?1"), [id], course_row)
        .optional()
        .map_err(err)?
        .ok_or_else(|| COURSE_NOT_FOUND.to_string())
}

fn clean_course(conn: &Connection, name: &str, note: Option<String>, except: Option<i64>) -> Result<(String, Option<String>), String> {
    let name = common::require(name, "课程名", 100)?;
    let note = common::clean(note);
    common::check_len(&note, "备注", 5000)?;
    let dup: i64 = conn
        .query_row(
            "SELECT count(*) FROM drawing_courses WHERE name = ?1 AND id != coalesce(?2, -1)",
            params![name, except],
            |r| r.get(0),
        )
        .map_err(err)?;
    if dup > 0 {
        return Err(format!("已经有叫「{name}」的课程了"));
    }
    Ok((name, note))
}

pub fn create_course(conn: &Connection, patch: CoursePatch) -> Result<Course, String> {
    let (name, note) = clean_course(conn, patch.name.as_deref().unwrap_or(""), patch.note.flatten(), None)?;
    conn.execute(
        "INSERT INTO drawing_courses (name, note, sort_order)
         VALUES (?1, ?2, (SELECT coalesce(max(sort_order), 0) + 1 FROM drawing_courses))",
        params![name, note],
    )
    .map_err(err)?;
    course(conn, conn.last_insert_rowid())
}

pub fn update_course(conn: &Connection, id: i64, patch: CoursePatch) -> Result<Course, String> {
    let old = course(conn, id)?;
    let (name, note) = clean_course(
        conn,
        patch.name.as_deref().unwrap_or(&old.name),
        patch.note.unwrap_or(old.note),
        Some(id),
    )?;
    conn.execute(
        &format!("UPDATE drawing_courses SET name = ?1, note = ?2, updated_at = {NOW} WHERE id = ?3"),
        params![name, note, id],
    )
    .map_err(err)?;
    course(conn, id)
}

/// Deletes the course and all its lessons (FK cascade).
pub fn delete_course(conn: &Connection, id: i64) -> Result<(), String> {
    match conn.execute("DELETE FROM drawing_courses WHERE id = ?1", [id]).map_err(err)? {
        0 => Err(COURSE_NOT_FOUND.into()),
        _ => Ok(()),
    }
}

// ---------- lessons ----------

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Lesson {
    pub id: i64,
    pub course_id: i64,
    /// Display-only number like "2B", "7S_1"; not an ordering key.
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

#[derive(Debug, Clone, Default, PartialEq)]
struct Fields {
    code: Option<String>,
    kind_id: Option<i64>,
    title: String,
    status: Status,
    finished_month: Option<String>,
    note: Option<String>,
}

impl Fields {
    fn from_lesson(l: &Lesson) -> Self {
        Self {
            code: l.code.clone(),
            kind_id: l.kind_id,
            title: l.title.clone(),
            status: l.status,
            finished_month: l.finished_month.clone(),
            note: l.note.clone(),
        }
    }

    fn apply(mut self, p: LessonPatch) -> Self {
        if let Some(v) = p.code {
            self.code = v;
        }
        if let Some(v) = p.kind_id {
            self.kind_id = v;
        }
        if let Some(v) = p.title {
            self.title = v;
        }
        if let Some(v) = p.status {
            self.status = v;
        }
        if let Some(v) = p.finished_month {
            self.finished_month = v;
        }
        if let Some(v) = p.note {
            self.note = v;
        }
        self
    }

    fn validated(self) -> Result<Self, String> {
        let f = Self {
            code: common::clean(self.code),
            kind_id: self.kind_id,
            title: common::require(&self.title, "课名", 200)?,
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

const LESSON_COLS: &str =
    "id, course_id, code, kind_id, title, status, finished_month, note, sort_order, created_at, updated_at";

fn lesson_row(r: &Row) -> rusqlite::Result<Lesson> {
    Ok(Lesson {
        id: r.get(0)?,
        course_id: r.get(1)?,
        code: r.get(2)?,
        kind_id: r.get(3)?,
        title: r.get(4)?,
        status: r.get(5)?,
        finished_month: r.get(6)?,
        note: r.get(7)?,
        sort_order: r.get(8)?,
        created_at: r.get(9)?,
        updated_at: r.get(10)?,
    })
}

pub const PACE_MONTHS: i64 = 6;

#[derive(Debug, Clone, PartialEq)]
pub struct Pace {
    /// Finished in the last PACE_MONTHS months.
    pub recent: i64,
    pub remaining: i64,
    /// Lessons per month.
    pub rate: f64,
    /// None: nothing finished lately, so no estimate. 0 = all done.
    pub months_left: Option<i64>,
    /// YYYY-MM it would be done in.
    pub finish: Option<String>,
}

/// `lessons`: one course's (or several); `today`: YYYY-MM-DD.
pub fn pace(lessons: &[Lesson], today: &str) -> Pace {
    pace_of(lessons.iter().map(|l| (l.status, l.finished_month.as_deref())), today)
}

/// The same over (status, finished month) pairs (循序渐进 too).
pub fn pace_of<'a>(lessons: impl IntoIterator<Item = (Status, Option<&'a str>)>, today: &str) -> Pace {
    let this = format!("{}-01", &today[..7]);
    let since = common::add_months(&this, 1 - PACE_MONTHS)[..7].to_string();
    let (mut recent, mut remaining) = (0i64, 0i64);
    for (status, month) in lessons {
        if status != Status::Done {
            remaining += 1;
        } else if month.is_some_and(|m| m >= since.as_str() && m <= &today[..7]) {
            recent += 1;
        }
    }
    let rate = recent as f64 / PACE_MONTHS as f64;
    let months_left = if remaining == 0 { Some(0) } else if recent == 0 { None } else { Some((remaining as f64 / rate).ceil() as i64) };
    Pace { recent, remaining, rate, months_left, finish: months_left.map(|m| common::add_months(&this, m)[..7].to_string()) }
}

pub fn lessons(conn: &Connection, course_id: i64) -> Result<Vec<Lesson>, String> {
    course(conn, course_id)?;
    let mut st = conn
        .prepare(&format!("SELECT {LESSON_COLS} FROM drawing_lessons WHERE course_id = ?1 ORDER BY sort_order, id"))
        .map_err(err)?;
    let rows = st.query_map([course_id], lesson_row).map_err(err)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(err)
}

pub fn lesson(conn: &Connection, id: i64) -> Result<Lesson, String> {
    conn.query_row(&format!("SELECT {LESSON_COLS} FROM drawing_lessons WHERE id = ?1"), [id], lesson_row)
        .optional()
        .map_err(err)?
        .ok_or_else(|| LESSON_NOT_FOUND.to_string())
}

/// Appends to the end of the course. A new lesson starts as 未开始 (todo) unless a status is given.
pub fn create_lesson(conn: &Connection, course_id: i64, patch: LessonPatch) -> Result<Lesson, String> {
    course(conn, course_id)?;
    let month_given = patch.finished_month.is_some();
    let mut f = Fields { status: Status::Todo, ..Fields::default() }.apply(patch).validated()?;
    kinds::check(conn, KEY, f.kind_id)?;
    common::auto_finished(conn, f.status, false, &mut f.finished_month, month_given, 7)?;
    conn.execute(
        "INSERT INTO drawing_lessons (course_id, code, kind_id, title, status, finished_month, note, sort_order)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7,
           (SELECT coalesce(max(sort_order), 0) + 1 FROM drawing_lessons WHERE course_id = ?1))",
        params![course_id, f.code, f.kind_id, f.title, f.status, f.finished_month, f.note],
    )
    .map_err(err)?;
    lesson(conn, conn.last_insert_rowid())
}

pub fn update_lesson(conn: &Connection, id: i64, patch: LessonPatch) -> Result<Lesson, String> {
    let old = lesson(conn, id)?;
    let month_given = patch.finished_month.is_some();
    let mut f = Fields::from_lesson(&old).apply(patch).validated()?;
    kinds::check(conn, KEY, f.kind_id)?;
    common::auto_finished(conn, f.status, old.status == Status::Done, &mut f.finished_month, month_given, 7)?;
    conn.execute(
        &format!(
            "UPDATE drawing_lessons SET code = ?1, kind_id = ?2, title = ?3, status = ?4, finished_month = ?5,
               note = ?6, updated_at = {NOW} WHERE id = ?7"
        ),
        params![f.code, f.kind_id, f.title, f.status, f.finished_month, f.note, id],
    )
    .map_err(err)?;
    lesson(conn, id)
}

pub fn delete_lesson(conn: &Connection, id: i64) -> Result<(), String> {
    match conn.execute("DELETE FROM drawing_lessons WHERE id = ?1", [id]).map_err(err)? {
        0 => Err(LESSON_NOT_FOUND.into()),
        _ => Ok(()),
    }
}

// ---------- import / export: nested courses → lessons, kinds by name ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LessonRecord {
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    pub title: String,
    #[serde(default)]
    pub status: Status,
    #[serde(default)]
    pub finished_month: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CourseRecord {
    pub name: String,
    #[serde(default)]
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
        Fields {
            code: self.code.clone(),
            kind_id: None,
            title: self.title.clone(),
            status: self.status,
            finished_month: self.finished_month.clone(),
            note: self.note.clone(),
        }
        .validated()
    }
}

pub fn count(conn: &Connection) -> Result<i64, String> {
    conn.query_row("SELECT count(*) FROM drawing_courses", [], |r| r.get(0)).map_err(err)
}

pub fn validate_records(records: &[CourseRecord]) -> Result<(), String> {
    let mut names = std::collections::HashSet::new();
    for (i, c) in records.iter().enumerate() {
        let name = common::require(&c.name, "课程名", 100).map_err(|e| format!("{LABEL}第 {} 门课程：{e}", i + 1))?;
        if !names.insert(name.clone()) {
            return Err(format!("{LABEL}：课程「{name}」重复了"));
        }
        for (j, l) in c.lessons.iter().enumerate() {
            l.fields()
                .map_err(|e| format!("{LABEL}「{name}」第 {} 节（{}）：{e}", j + 1, l.title.trim()))?;
        }
    }
    Ok(())
}

/// Courses → lessons in display order (kinds by name), for the export file.
pub fn export(conn: &Connection) -> Result<Vec<CourseRecord>, String> {
    let names = kinds::name_map(conn, KEY)?;
    courses(conn)?
        .into_iter()
        .map(|c| {
            let lessons = lessons(conn, c.id)?
                .into_iter()
                .map(|l| LessonRecord {
                    code: l.code,
                    kind: l.kind_id.and_then(|id| names.get(&id).cloned()),
                    title: l.title,
                    status: l.status,
                    finished_month: l.finished_month,
                    note: l.note,
                    created_at: Some(l.created_at),
                    updated_at: Some(l.updated_at),
                })
                .collect();
            Ok(CourseRecord { name: c.name, note: c.note, lessons, created_at: Some(c.created_at), updated_at: Some(c.updated_at) })
        })
        .collect()
}

/// Replaces all courses + lessons (as-is, no auto-fill). Caller owns the transaction.
pub fn replace_all(conn: &Connection, records: &[CourseRecord]) -> Result<usize, String> {
    validate_records(records)?;
    conn.execute("DELETE FROM drawing_courses", []).map_err(err)?; // lessons cascade
    for (i, c) in records.iter().enumerate() {
        conn.execute(
            &format!(
                "INSERT INTO drawing_courses (name, note, sort_order, created_at, updated_at)
                 VALUES (?1, ?2, ?3, coalesce(?4, {NOW}), coalesce(?5, ?4, {NOW}))"
            ),
            params![c.name.trim(), common::clean(c.note.clone()), i as i64 + 1, c.created_at, c.updated_at],
        )
        .map_err(err)?;
        let course_id = conn.last_insert_rowid();
        for (j, l) in c.lessons.iter().enumerate() {
            let f = l.fields()?;
            let kind_id = kinds::resolve(conn, KEY, l.kind.as_deref())?;
            conn.execute(
                &format!(
                    "INSERT INTO drawing_lessons
                       (course_id, code, kind_id, title, status, finished_month, note, sort_order, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, coalesce(?9, {NOW}), coalesce(?10, ?9, {NOW}))"
                ),
                params![
                    course_id, f.code, kind_id, f.title, f.status, f.finished_month, f.note, j as i64 + 1, l.created_at,
                    l.updated_at
                ],
            )
            .map_err(err)?;
        }
    }
    Ok(records.len())
}

// ---------- commands ----------

#[tauri::command]
pub fn drawing_courses_reorder(state: State<AppState>, ids: Vec<i64>) -> Result<(), String> {
    state.with_undo("调整顺序", |c| common::reorder(c, "drawing_courses", "1 = 1", &[], &ids))
}

#[tauri::command]
pub fn drawing_lessons_reorder(state: State<AppState>, course_id: i64, ids: Vec<i64>) -> Result<(), String> {
    state.with_undo("调整顺序", |c| common::reorder(c, "drawing_lessons", "course_id = ?1", &[&course_id], &ids))
}

#[tauri::command]
pub fn drawing_courses(state: State<AppState>) -> Result<Vec<Course>, String> {
    state.with_db(|c| courses(c))
}

#[tauri::command]
pub fn drawing_course_create(state: State<AppState>, patch: CoursePatch) -> Result<Course, String> {
    state.with_undo("添加课程", |c| create_course(c, patch))
}

#[tauri::command]
pub fn drawing_course_update(state: State<AppState>, id: i64, patch: CoursePatch) -> Result<Course, String> {
    state.with_undo("修改课程", |c| update_course(c, id, patch))
}

#[tauri::command]
pub fn drawing_course_delete(state: State<AppState>, id: i64) -> Result<(), String> {
    state.with_undo("删除课程", |c| delete_course(c, id))
}

#[tauri::command]
pub fn drawing_lessons(state: State<AppState>, course_id: i64) -> Result<Vec<Lesson>, String> {
    state.with_db(|c| lessons(c, course_id))
}

#[tauri::command]
pub fn drawing_lesson_create(state: State<AppState>, course_id: i64, patch: LessonPatch) -> Result<Lesson, String> {
    state.with_undo("添加课节", |c| create_lesson(c, course_id, patch))
}

#[tauri::command]
pub fn drawing_lesson_update(state: State<AppState>, id: i64, patch: LessonPatch) -> Result<Lesson, String> {
    state.with_undo("修改课节", |c| update_lesson(c, id, patch))
}

#[tauri::command]
pub fn drawing_lesson_delete(state: State<AppState>, id: i64) -> Result<(), String> {
    state.with_undo("删除课节", |c| delete_lesson(c, id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::memory_db;
    use crate::modules::kinds::tests::kind_id;
    use serde_json::json;

    fn cp(v: serde_json::Value) -> CoursePatch {
        serde_json::from_value(v).unwrap()
    }
    fn lp(v: serde_json::Value) -> LessonPatch {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn courses_crud_and_unique_names() {
        let c = memory_db();
        let a = create_course(&c, cp(json!({"name": " 人体前传 "}))).unwrap();
        assert_eq!((a.name.as_str(), a.sort_order, a.total), ("人体前传", 1, 0));
        let b = create_course(&c, cp(json!({"name": "人体正传", "note": "下半年"}))).unwrap();
        assert_eq!(b.sort_order, 2);
        assert!(create_course(&c, cp(json!({"name": "人体前传"}))).unwrap_err().contains("已经有"));
        assert!(create_course(&c, cp(json!({}))).is_err());
        let b = update_course(&c, b.id, cp(json!({"note": null}))).unwrap();
        assert_eq!((b.name.as_str(), b.note), ("人体正传", None));
        assert!(update_course(&c, b.id, cp(json!({"name": "人体前传"}))).is_err());
        assert_eq!(courses(&c).unwrap().len(), 2);
    }

    #[test]
    fn lessons_append_progress_and_cascade() {
        let c = memory_db();
        let now = common::current_month(&c).unwrap();
        let course_id = create_course(&c, cp(json!({"name": "原创绘画全攻略"}))).unwrap().id;
        let main = kind_id(&c, "drawing", "正课");
        let l1 = create_lesson(&c, course_id, lp(json!({"title": "绘画大地图", "code": "0", "kindId": main, "status": "todo"}))).unwrap();
        let l2 = create_lesson(&c, course_id, lp(json!({"title": "跟画：线条", "code": " "}))).unwrap();
        assert_eq!((l1.sort_order, l2.sort_order, l2.code.clone()), (1, 2, None));
        assert_eq!(l2.status, Status::Todo); // new lessons start as 未开始
        let l3 = create_lesson(&c, course_id, lp(json!({"title": "待定", "status": "unmarked"}))).unwrap();
        assert_eq!(l3.status, Status::Todo); // no 待标记 in 随意画画
        delete_lesson(&c, l3.id).unwrap();
        assert!(create_lesson(&c, course_id, lp(json!({"title": "x", "kindId": kind_id(&c, "books", "虚构")}))).is_err());
        assert!(create_lesson(&c, 999, lp(json!({"title": "x"}))).is_err());

        let u = update_lesson(&c, l1.id, lp(json!({"status": "done"}))).unwrap();
        assert_eq!(u.finished_month.as_deref(), Some(now.as_str()));
        update_lesson(&c, l2.id, lp(json!({"status": "doing"}))).unwrap();
        let co = course(&c, course_id).unwrap();
        assert_eq!((co.total, co.done, co.doing), (2, 1, 1));
        assert_eq!(lessons(&c, course_id).unwrap().iter().map(|l| l.id).collect::<Vec<_>>(), [l1.id, l2.id]);

        delete_lesson(&c, l2.id).unwrap();
        assert!(delete_lesson(&c, l2.id).is_err());
        delete_course(&c, course_id).unwrap();
        let n: i64 = c.query_row("SELECT count(*) FROM drawing_lessons", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn replace_all_nested_with_kind_names() {
        let c = memory_db();
        create_course(&c, cp(json!({"name": "old"}))).unwrap();
        let recs: Vec<CourseRecord> = serde_json::from_value(json!([
            {"name": "A", "lessons": [
                {"code": "0", "kind": "正课", "title": "第一课", "status": "todo"},
                {"kind": "DLC 跟画", "title": "跟画", "status": "done"},
                {"kind": "新类型", "title": "加餐"}
            ]},
            {"name": "B"}
        ]))
        .unwrap();
        assert_eq!(replace_all(&c, &recs).unwrap(), 2);
        let cs = courses(&c).unwrap();
        assert_eq!(cs.iter().map(|x| (x.name.as_str(), x.total, x.done)).collect::<Vec<_>>(), [("A", 3, 1), ("B", 0, 0)]);
        let ls = lessons(&c, cs[0].id).unwrap();
        assert_eq!(ls[0].kind_id, Some(kind_id(&c, "drawing", "正课")));
        assert_eq!(ls[1].finished_month, None); // as-is
        assert_eq!(ls[2].kind_id, Some(kind_id(&c, "drawing", "新类型")));

        let dup: Vec<CourseRecord> = serde_json::from_value(json!([{"name": "X"}, {"name": " X "}])).unwrap();
        assert!(validate_records(&dup).unwrap_err().contains("重复"));
        let bad: Vec<CourseRecord> = serde_json::from_value(json!([{"name": "X", "lessons": [{"title": ""}]}])).unwrap();
        assert!(validate_records(&bad).unwrap_err().contains("第 1 节"));
    }
}
