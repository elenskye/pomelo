//! 玩物有志 (games, v1.0 Phase 5): a sibling of books.rs / films.rs (same table and panel), keep them in step.
//! - 类型 = kinds.rs module "games", 平台 = kinds.rs module "game_platform" (games.platform_id); both user-editable.
//! - 状态: 想玩 todo · 在玩 doing · 通关 done (no 待标记; new ones are 想玩). Becoming 通关 with no 完成时间 fills this year.
//! - 周目: the first time through is the game's own 完成时间 (finished_year) and 尝尝咸淡; 二周目 on are `replays`
//!   (year + rating each, like 重读). The table's 完成 is the latest year, 周目 how many times through.
//! - Renaming a game rewrites the links to it (`[[玩物有志/名字]]`) in the same step (links.rs).
//! - 搬过来 (`from_list`): a 五花八门 list's items become games, fields matched by name (名称, 类型, 平台, 国家, 状态,
//!   尝尝咸淡 / 评分, 完成 / 完成年份, 备注); a value that fits nowhere goes into 备注 as「字段：值」, a 状态 other than
//!   想玩 / 在玩 / 通关 too (常青 counts as 在玩). Then the list is deleted. One ⌘Z step.

use super::common::{self, err, some, Status};
use super::{custom, kinds, links, lists};
use crate::AppState;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use tauri::State;

pub const KEY: &str = "games";
pub const LABEL: &str = "玩物有志";
pub const PLATFORMS: &str = "game_platform";
const REPLAYS: (&str, &str) = ("game_replays", "game_id");
const NOT_FOUND: &str = "找不到这个游戏，可能已经被删除了";

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Game {
    pub id: i64,
    pub title: String,
    /// kinds.id in 平台; None = 没填.
    pub platform_id: Option<i64>,
    pub country: Option<String>,
    /// kinds.id in 类型; None = 未分类.
    pub kind_id: Option<i64>,
    pub status: Status,
    pub rating: Option<i64>,
    pub finished_year: Option<String>,
    pub note: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    /// 二周目 on, in order.
    pub replays: Vec<common::Reread>,
    /// How many times through (the table's 周目; `runs`).
    pub runs: usize,
}

/// Caller-settable fields. Absent = keep (update) / default (create); explicit null clears.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GamePatch {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default, deserialize_with = "some")]
    pub platform_id: Option<Option<i64>>,
    #[serde(default, deserialize_with = "some")]
    pub country: Option<Option<String>>,
    #[serde(default, deserialize_with = "some")]
    pub kind_id: Option<Option<i64>>,
    #[serde(default)]
    pub status: Option<Status>,
    #[serde(default, deserialize_with = "some")]
    pub rating: Option<Option<i64>>,
    #[serde(default, deserialize_with = "some")]
    pub finished_year: Option<Option<String>>,
    #[serde(default, deserialize_with = "some")]
    pub note: Option<Option<String>>,
    /// The whole list (replaces).
    #[serde(default)]
    pub replays: Option<Vec<common::Reread>>,
}

#[derive(Debug, Clone, PartialEq)]
struct Fields {
    title: String,
    platform_id: Option<i64>,
    country: Option<String>,
    kind_id: Option<i64>,
    status: Status,
    rating: Option<i64>,
    finished_year: Option<String>,
    note: Option<String>,
    replays: Vec<common::Reread>,
}

impl Default for Fields {
    fn default() -> Self {
        Self { title: String::new(), platform_id: None, country: None, kind_id: None, status: Status::Todo, rating: None, finished_year: None, note: None, replays: Vec::new() }
    }
}

impl Fields {
    fn from_game(g: &Game) -> Self {
        Self {
            title: g.title.clone(),
            platform_id: g.platform_id,
            country: g.country.clone(),
            kind_id: g.kind_id,
            status: g.status,
            rating: g.rating,
            finished_year: g.finished_year.clone(),
            note: g.note.clone(),
            replays: g.replays.clone(),
        }
    }

    fn apply(mut self, p: GamePatch) -> Self {
        if let Some(v) = p.title {
            self.title = v;
        }
        if let Some(v) = p.platform_id {
            self.platform_id = v;
        }
        if let Some(v) = p.country {
            self.country = v;
        }
        if let Some(v) = p.kind_id {
            self.kind_id = v;
        }
        if let Some(v) = p.status {
            self.status = v;
        }
        if let Some(v) = p.rating {
            self.rating = v;
        }
        if let Some(v) = p.finished_year {
            self.finished_year = v;
        }
        if let Some(v) = p.note {
            self.note = v;
        }
        if let Some(v) = p.replays {
            self.replays = v;
        }
        self
    }

    fn validated(self) -> Result<Self, String> {
        if self.status == Status::Unmarked {
            return Err("状态只有想玩、在玩、通关".into());
        }
        let f = Self {
            title: common::require(&self.title, "名称", 200)?,
            platform_id: self.platform_id,
            country: common::clean(self.country),
            kind_id: self.kind_id,
            status: self.status,
            rating: self.rating,
            finished_year: common::clean(self.finished_year),
            note: common::clean(self.note),
            replays: common::clean_rereads(self.replays)?,
        };
        common::check_len(&f.country, "国家", 50)?;
        common::check_len(&f.note, "备注", 5000)?;
        common::check_rating(f.rating)?;
        common::check_year(&f.finished_year)?;
        Ok(f)
    }
}

const COLS: &str = "id, title, platform_id, country, kind_id, status, rating, finished_year, note, created_at, updated_at";

fn from_row(r: &Row) -> rusqlite::Result<Game> {
    Ok(Game {
        id: r.get(0)?,
        title: r.get(1)?,
        platform_id: r.get(2)?,
        country: r.get(3)?,
        kind_id: r.get(4)?,
        status: r.get(5)?,
        rating: r.get(6)?,
        finished_year: r.get(7)?,
        note: r.get(8)?,
        created_at: r.get(9)?,
        updated_at: r.get(10)?,
        replays: Vec::new(),
        runs: 0,
    })
}

pub fn list(conn: &Connection) -> Result<Vec<Game>, String> {
    let mut st = conn.prepare(&format!("SELECT {COLS} FROM games ORDER BY id")).map_err(err)?;
    let mut rr = common::load_rereads(conn, REPLAYS.0, REPLAYS.1, None)?;
    let rows = st.query_map([], from_row).map_err(err)?.collect::<Result<Vec<Game>, _>>().map_err(err)?;
    Ok(rows.into_iter().map(|x| with_runs(Game { replays: rr.remove(&x.id).unwrap_or_default(), ..x })).collect())
}

pub fn get(conn: &Connection, id: i64) -> Result<Game, String> {
    conn.query_row(&format!("SELECT {COLS} FROM games WHERE id = ?1"), [id], from_row)
        .optional()
        .map_err(err)?
        .ok_or_else(|| NOT_FOUND.to_string())
        .and_then(|x| Ok(with_runs(Game { replays: common::load_rereads(conn, REPLAYS.0, REPLAYS.1, Some(id))?.remove(&id).unwrap_or_default(), ..x })))
}

pub fn count(conn: &Connection) -> Result<i64, String> {
    conn.query_row("SELECT count(*) FROM games", [], |r| r.get(0)).map_err(err)
}

/// How many times through: the first (通关 or a 完成时间) and each 周目 after it.
fn with_runs(g: Game) -> Game {
    let first = g.status == Status::Done || g.finished_year.is_some() || !g.replays.is_empty();
    Game { runs: usize::from(first) + g.replays.len(), ..g }
}

fn check_refs(conn: &Connection, f: &Fields) -> Result<(), String> {
    kinds::check(conn, KEY, f.kind_id)?;
    match f.platform_id {
        Some(_) => kinds::check(conn, PLATFORMS, f.platform_id).map_err(|_| "选择的平台不存在，可能已经被删除了".to_string()),
        None => Ok(()),
    }
}

fn insert(conn: &Connection, f: &Fields) -> Result<i64, String> {
    conn.execute(
        "INSERT INTO games (title, platform_id, country, kind_id, status, rating, finished_year, note)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![f.title, f.platform_id, f.country, f.kind_id, f.status, f.rating, f.finished_year, f.note],
    )
    .map_err(err)?;
    let id = conn.last_insert_rowid();
    common::save_rereads(conn, REPLAYS.0, REPLAYS.1, id, &f.replays)?;
    Ok(id)
}

pub fn create(conn: &Connection, patch: GamePatch) -> Result<Game, String> {
    let year_given = patch.finished_year.is_some();
    let mut f = Fields::default().apply(patch).validated()?;
    check_refs(conn, &f)?;
    common::auto_finished(conn, f.status, false, &mut f.finished_year, year_given, 4)?;
    let tx = common::Tx::begin(conn)?;
    let id = insert(&tx, &f)?;
    tx.commit().map_err(err)?;
    get(conn, id)
}

pub fn update(conn: &Connection, id: i64, patch: GamePatch) -> Result<Game, String> {
    let old = get(conn, id)?;
    let year_given = patch.finished_year.is_some();
    let mut f = Fields::from_game(&old).apply(patch).validated()?;
    check_refs(conn, &f)?;
    common::auto_finished(conn, f.status, old.status == Status::Done, &mut f.finished_year, year_given, 4)?;
    let tx = common::Tx::begin(conn)?;
    tx.execute(
        "UPDATE games SET title = ?1, platform_id = ?2, country = ?3, kind_id = ?4, status = ?5, rating = ?6,
           finished_year = ?7, note = ?8, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?9",
        params![f.title, f.platform_id, f.country, f.kind_id, f.status, f.rating, f.finished_year, f.note, id],
    )
    .map_err(err)?;
    if f.replays != old.replays {
        common::save_rereads(&tx, REPLAYS.0, REPLAYS.1, id, &f.replays)?;
    }
    if f.title != old.title {
        links::follow_rename(&tx, KEY, &old.title, &f.title)?;
    }
    tx.commit().map_err(err)?;
    get(conn, id)
}

pub fn delete(conn: &Connection, id: i64) -> Result<(), String> {
    match conn.execute("DELETE FROM games WHERE id = ?1", [id]).map_err(err)? {
        0 => Err(NOT_FOUND.into()),
        _ => Ok(()),
    }
}

// ---------- 搬过来：a 五花八门 list → games ----------

/// What moving a list would do: how many items, and which of its fields go where.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Move {
    pub list_id: i64,
    pub name: String,
    pub items: usize,
    /// (the list's field, where it goes: 类型 / 平台 / … / 备注).
    pub fields: Vec<(String, String)>,
}

fn target_of(name: &str, ty: lists::FieldType) -> &'static str {
    use lists::FieldType as T;
    match (name.trim(), ty) {
        ("类型" | "分类", T::Select | T::Multi | T::Text) => "类型",
        ("平台" | "机器" | "主机", T::Select | T::Multi | T::Text) => "平台",
        ("国家" | "地区" | "国家/地区", T::Text | T::Select) => "国家",
        ("状态", T::Select | T::Text) => "状态",
        (_, T::Rating) => "尝尝咸淡",
        ("完成" | "完成年份" | "完成时间" | "通关" | "通关年份" | "年份", T::Year | T::Number | T::Text) => "完成时间",
        ("备注" | "笔记" | "感想", T::Text | T::LongText) => "备注",
        _ => "备注",
    }
}

fn list_fields(conn: &Connection, list_id: i64) -> Result<Vec<lists::Field>, String> {
    custom::of(conn, custom::Template::List, list_id)?;
    lists::fields(conn, list_id)
}

pub fn move_preview(conn: &Connection, list_id: i64) -> Result<Move, String> {
    let m = custom::of(conn, custom::Template::List, list_id)?;
    let fields = list_fields(conn, list_id)?;
    let items = lists::items(conn, list_id)?.len();
    Ok(Move { list_id, name: m.name, items, fields: fields.iter().map(|f| (f.name.clone(), target_of(&f.name, f.kind).to_string())).collect() })
}

fn status_of(v: &str) -> (Status, bool) {
    match v.trim() {
        "想玩" => (Status::Todo, true),
        "在玩" => (Status::Doing, true),
        "通关" | "通关了" | "玩完" => (Status::Done, true),
        "常青" => (Status::Doing, false),
        _ => (Status::Todo, false),
    }
}

fn text_of(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::String(s) => Some(s.trim().to_string()).filter(|s| !s.is_empty()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Bool(true) => Some("是".into()),
        serde_json::Value::Array(a) => Some(a.iter().filter_map(text_of).collect::<Vec<_>>().join("、")).filter(|s| !s.is_empty()),
        _ => None,
    }
}

/// Moves every item of the list into 玩物有志 (after the games already there), then deletes the list.
/// Returns how many games were added. Caller wraps it in one undo step.
pub fn from_list(conn: &Connection, list_id: i64) -> Result<usize, String> {
    let fields = list_fields(conn, list_id)?;
    let items = lists::items(conn, list_id)?;
    let tx = common::Tx::begin(conn)?;
    for it in &items {
        let mut f = Fields { title: it.name.clone(), ..Fields::default() };
        let mut extra: Vec<String> = Vec::new();
        let mut note: Option<String> = None;
        for fd in &fields {
            let Some(v) = it.values.get(&fd.id.to_string()) else { continue };
            let Some(text) = text_of(v) else { continue };
            match target_of(&fd.name, fd.kind) {
                "类型" => f.kind_id = kinds::resolve(&tx, KEY, text.split('、').next())?,
                "平台" => f.platform_id = kinds::resolve(&tx, PLATFORMS, text.split('、').next())?,
                "国家" => f.country = Some(text),
                "状态" => {
                    let (s, known) = status_of(&text);
                    f.status = s;
                    if !known {
                        extra.push(format!("{}：{text}", fd.name));
                    }
                }
                "尝尝咸淡" => f.rating = v.as_i64().filter(|r| (1..=5).contains(r)),
                "完成时间" => match common::year_of(Some(text.clone())) {
                    Some(y) if common::check_year(&Some(y.clone())).is_ok() => f.finished_year = Some(y),
                    _ => extra.push(format!("{}：{text}", fd.name)),
                },
                _ if note.is_none() && matches!(fd.name.trim(), "备注" | "笔记" | "感想") => note = Some(text),
                _ => extra.push(format!("{}：{text}", fd.name)),
            }
        }
        f.note = match (note, extra.is_empty()) {
            (n, true) => n,
            (Some(n), false) => Some(format!("{n}\n{}", extra.join("\n"))),
            (None, false) => Some(extra.join("\n")),
        };
        let f = f.validated().map_err(|e| format!("「{}」：{e}", it.name))?;
        insert(&tx, &f)?;
    }
    custom::delete(&tx, list_id)?;
    tx.commit().map_err(err)?;
    Ok(items.len())
}

// ---------- import / export (no ids; 类型 and 平台 by name) ----------

fn todo() -> Status {
    Status::Todo
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GameRecord {
    pub title: String,
    /// 平台 name (created if missing on import).
    #[serde(default)]
    pub platform: Option<String>,
    #[serde(default)]
    pub country: Option<String>,
    /// 类型 name (created if missing on import).
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default = "todo")]
    pub status: Status,
    #[serde(default)]
    pub rating: Option<i64>,
    #[serde(default)]
    pub finished_year: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub replays: Vec<common::Reread>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

impl GameRecord {
    fn fields(&self) -> Result<Fields, String> {
        kinds::check_name(&self.kind)?;
        kinds::check_name(&self.platform)?;
        Fields {
            title: self.title.clone(),
            platform_id: None,
            country: self.country.clone(),
            kind_id: None, // resolved from names in replace_all
            status: self.status,
            rating: self.rating,
            finished_year: common::year_of(self.finished_year.clone()),
            note: self.note.clone(),
            replays: self.replays.clone(),
        }
        .validated()
    }
}

fn record_err(i: usize, r: &GameRecord, e: String) -> String {
    format!("{LABEL}第 {} 条（{}）：{e}", i + 1, r.title.trim())
}

pub fn validate_records(records: &[GameRecord]) -> Result<(), String> {
    for (i, r) in records.iter().enumerate() {
        r.fields().map_err(|e| record_err(i, r, e))?;
    }
    Ok(())
}

pub fn export(conn: &Connection) -> Result<Vec<GameRecord>, String> {
    let kinds_by = kinds::name_map(conn, KEY)?;
    let platforms = kinds::name_map(conn, PLATFORMS)?;
    Ok(list(conn)?
        .into_iter()
        .map(|x| GameRecord {
            title: x.title,
            platform: x.platform_id.and_then(|id| platforms.get(&id).cloned()),
            country: x.country,
            kind: x.kind_id.and_then(|id| kinds_by.get(&id).cloned()),
            status: x.status,
            rating: x.rating,
            finished_year: x.finished_year,
            note: x.note,
            replays: x.replays,
            created_at: Some(x.created_at),
            updated_at: Some(x.updated_at),
        })
        .collect())
}

/// Replaces all games with `records` (as-is, no auto-fill). Caller owns the transaction.
pub fn replace_all(conn: &Connection, records: &[GameRecord]) -> Result<usize, String> {
    conn.execute("DELETE FROM games", []).map_err(err)?;
    let mut st = conn
        .prepare(
            "INSERT INTO games (title, platform_id, country, kind_id, status, rating, finished_year, note, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8,
               coalesce(?9, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
               coalesce(?10, ?9, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')))",
        )
        .map_err(err)?;
    for (i, r) in records.iter().enumerate() {
        let f = r.fields().map_err(|e| record_err(i, r, e))?;
        let kind_id = kinds::resolve(conn, KEY, r.kind.as_deref())?;
        let platform_id = kinds::resolve(conn, PLATFORMS, r.platform.as_deref())?;
        st.execute(params![f.title, platform_id, f.country, kind_id, f.status, f.rating, f.finished_year, f.note, r.created_at, r.updated_at])
            .map_err(err)?;
        common::save_rereads(conn, REPLAYS.0, REPLAYS.1, conn.last_insert_rowid(), &f.replays)?;
    }
    Ok(records.len())
}

// ---------- commands ----------

#[tauri::command]
pub fn games_list(state: State<AppState>) -> Result<Vec<Game>, String> {
    state.with_db(|c| list(c))
}

#[tauri::command]
pub fn games_create(state: State<AppState>, patch: GamePatch) -> Result<Game, String> {
    state.with_undo("添加游戏", |c| create(c, patch))
}

#[tauri::command]
pub fn games_update(state: State<AppState>, id: i64, patch: GamePatch) -> Result<Game, String> {
    state.with_undo("修改游戏", |c| update(c, id, patch))
}

#[tauri::command]
pub fn games_delete(state: State<AppState>, id: i64) -> Result<(), String> {
    state.with_undo("删除游戏", |c| delete(c, id))
}

#[tauri::command]
pub fn games_move_preview(state: State<AppState>, list_id: i64) -> Result<Move, String> {
    state.with_db(|c| move_preview(c, list_id))
}

#[tauri::command]
pub fn games_from_list(state: State<AppState>, list_id: i64) -> Result<usize, String> {
    state.with_undo("搬到玩物有志", |c| from_list(c, list_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::memory_db;
    use crate::modules::kinds::tests::kind_id;
    use serde_json::json;

    fn patch(v: serde_json::Value) -> GamePatch {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn create_defaults_checks_and_runs() {
        let c = memory_db();
        let g = create(&c, patch(json!({"title": " 风之旅人 ", "country": "美国"}))).unwrap();
        assert_eq!((g.title.as_str(), g.status, g.platform_id, g.runs), ("风之旅人", Status::Todo, None, 0));
        assert!(create(&c, patch(json!({"title": "x", "status": "unmarked"}))).is_err());
        let ps5 = kind_id(&c, "game_platform", "PS5");
        let act = kind_id(&c, "games", "动作");
        assert!(create(&c, patch(json!({"title": "x", "platformId": act}))).is_err()); // a 类型 isn't a 平台
        assert!(create(&c, patch(json!({"title": "x", "kindId": ps5}))).is_err());
        let now = common::current_month(&c).unwrap()[..4].to_string();
        let d = create(&c, patch(json!({"title": "哈迪斯", "status": "done", "platformId": ps5, "kindId": act, "rating": 4}))).unwrap();
        assert_eq!((d.finished_year.as_deref(), d.platform_id, d.runs), (Some(now.as_str()), Some(ps5), 1));
        let d = update(&c, d.id, patch(json!({"replays": [{"year": "2026", "rating": 5}]}))).unwrap();
        assert_eq!(d.runs, 2);
        // deleting the platform leaves it empty
        kinds::delete(&c, ps5).unwrap();
        assert_eq!(get(&c, d.id).unwrap().platform_id, None);
        delete(&c, d.id).unwrap();
        assert_eq!(c.query_row("SELECT count(*) FROM game_replays", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
    }

    #[test]
    fn export_import_round_trip() {
        let c = memory_db();
        let sw = kind_id(&c, "game_platform", "Switch");
        create(&c, patch(json!({"title": "塞尔达传说：王国之泪", "platformId": sw, "status": "done", "finishedYear": "2023", "rating": 5, "replays": [{"year": "2026"}], "note": "呀哈哈"}))).unwrap();
        create(&c, patch(json!({"title": "星露谷物语", "status": "doing"}))).unwrap();
        let out = export(&c).unwrap();
        assert_eq!(out[0].platform.as_deref(), Some("Switch"));
        let c2 = memory_db();
        replace_all(&c2, &out).unwrap();
        assert_eq!(serde_json::to_value(&out).unwrap(), serde_json::to_value(export(&c2).unwrap()).unwrap());
        // a new 平台 by name comes with it
        let recs: Vec<GameRecord> = serde_json::from_value(json!([{"title": "A", "platform": "Steam Deck"}])).unwrap();
        replace_all(&c2, &recs).unwrap();
        assert_eq!(list(&c2).unwrap()[0].platform_id, Some(kind_id(&c2, "game_platform", "Steam Deck")));
        assert!(validate_records(&serde_json::from_value::<Vec<GameRecord>>(json!([{"title": " "}])).unwrap()).is_err());
    }

    #[test]
    fn moving_a_list_matches_fields_by_name() {
        let c = memory_db();
        let m = custom::create(&c, serde_json::from_value(json!({"template": "list", "name": "游戏", "icon": "gamepad-2"})).unwrap()).unwrap();
        let f = |name: &str, ty: &str, options: serde_json::Value| {
            let options: Vec<serde_json::Value> = options.as_array().unwrap().iter().map(|o| json!({"name": o})).collect();
            lists::create_field(&c, m.id, serde_json::from_value(json!({"name": name, "type": ty, "options": options})).unwrap()).unwrap().id
        };
        let kind = f("类型", "select", json!(["剧情", "双人成行"]));
        let country = f("国家", "text", json!([]));
        let status = f("状态", "select", json!(["想玩", "在玩", "通关", "常青"]));
        let rating = f("尝尝咸淡", "rating", json!([]));
        let done = f("完成", "year", json!([]));
        let note = f("备注", "longtext", json!([]));
        let mood = f("心情", "text", json!([]));
        let item = |name: &str, values: serde_json::Value| lists::create_item(&c, m.id, serde_json::from_value(json!({"name": name, "values": values})).unwrap()).unwrap();
        item("双人成行", json!({kind.to_string(): "双人成行", country.to_string(): "瑞典", status.to_string(): "通关", rating.to_string(): 4, done.to_string(): "2025", note.to_string(): "和小林一起"}));
        item("动物森友会", json!({status.to_string(): "常青", mood.to_string(): "放松"}));
        let p = move_preview(&c, m.id).unwrap();
        assert_eq!(p.items, 2);
        assert_eq!(p.fields.iter().map(|(a, b)| format!("{a}→{b}")).collect::<Vec<_>>(), ["类型→类型", "国家→国家", "状态→状态", "尝尝咸淡→尝尝咸淡", "完成→完成时间", "备注→备注", "心情→备注"]);
        assert_eq!(from_list(&c, m.id).unwrap(), 2);
        let all = list(&c).unwrap();
        assert_eq!(all[0].title, "双人成行");
        assert_eq!((all[0].status, all[0].rating, all[0].finished_year.as_deref(), all[0].country.as_deref(), all[0].note.as_deref()), (Status::Done, Some(4), Some("2025"), Some("瑞典"), Some("和小林一起")));
        assert_eq!(all[0].kind_id, Some(kind_id(&c, "games", "双人成行"))); // a 类型 the list had is added
        assert_eq!((all[1].status, all[1].note.as_deref()), (Status::Doing, Some("状态：常青\n心情：放松")));
        assert!(custom::of(&c, custom::Template::List, m.id).is_err()); // the list is gone
    }
}
