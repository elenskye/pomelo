//! Assembling the draft from materials, a material's newer version, confirmed versions, sorting.
//! 增加 appends the chosen materials not in the draft yet (text, order and header stay); 重新组装 and
//! 新建 start over from 基本信息 + 教育 (+ the chosen ones). Each is one undo step.
use super::{catalog::Record, *};
use crate::modules::common::Tx;

/// Where an entry came from: the material, the revision it had, the points chosen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub record_id: String,
    pub revision: i64,
    pub point_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Selection {
    pub record_id: String,
    pub point_ids: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceChange {
    pub entry_id: String,
    pub title: String,
    pub before: Entry,
    pub after: Entry,
}

/// 页首 by default, in this order; the profile's other fields come after, hidden.
pub const HEADER: [&str; 4] = ["工作邮箱", "联系电话", "个人网站", "工作地点"];

pub fn id() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|e| e.to_string())?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// 技术栈 lines: chosen by default only for 纯开发岗.
pub fn is_stack(p: &Point) -> bool {
    let t = p.text.trim_start_matches(['*', ' ']).to_lowercase();
    t.starts_with("技术栈") || t.starts_with("tech stack")
}

fn entry(record: &Record, chosen: &[String], entry_id: String) -> Entry {
    let points = record.points.iter().filter(|p| chosen.contains(&p.id)).map(|p| Point { id: format!("{entry_id}:{}", p.id), text: p.text.clone() }).collect();
    let source = Source { record_id: record.id.clone(), revision: record.revision, point_ids: chosen.to_vec() };
    Entry { id: entry_id, time: record.time.clone(), name: record.title.clone(), role: record.role.clone(), location: record.location.clone(), note: record.text.clone(), points, source: Some(source) }
}

fn section_title(category: &str) -> String {
    match category {
        "教育" => "教育背景".into(),
        "技能" => "专业技能".into(),
        c => format!("{c}经历"),
    }
}

pub fn initial(records: &[Record]) -> Result<Draft, String> {
    let profile = records.iter().find(|r| r.kind == "profile" && r.enabled);
    let value = |label: &str| profile.and_then(|p| p.fields.iter().find(|f| f.label == label)).map(|f| f.value.clone()).unwrap_or_default();
    let mut header: Vec<HeaderField> = HEADER.iter().enumerate().map(|(i, label)| HeaderField { id: format!("header-{i}"), label: (*label).into(), value: value(label), visible: true }).collect();
    for f in profile.map(|p| p.fields.as_slice()).unwrap_or_default() {
        if f.label != "姓名" && !HEADER.contains(&f.label.as_str()) {
            header.push(HeaderField { id: format!("header-{}", header.len()), label: f.label.clone(), value: f.value.clone(), visible: false });
        }
    }
    let education = records.iter().filter(|r| r.kind == "material" && r.category == "教育" && r.enabled).map(|r| Ok(entry(r, &r.points.iter().map(|p| p.id.clone()).collect::<Vec<_>>(), id()?))).collect::<Result<_, String>>()?;
    Ok(Draft { name: value("姓名"), header, sections: vec![Section { id: id()?, title: section_title("教育"), category: "教育".into(), entries: education }], jd: String::new() })
}

pub fn baseline(c: &Connection, draft: &Draft) -> Result<(), String> {
    c.execute("UPDATE seeker_draft SET baseline = ?1 WHERE id = 1", [serde_json::to_string(draft).map_err(err)?]).map_err(err)?;
    Ok(())
}

/// The draft with the chosen materials added (replace: a new draft keeping only the JD).
pub fn assemble(records: &[Record], current: Option<Draft>, selections: &[Selection], replace: bool) -> Result<Draft, String> {
    let mut draft = match current {
        Some(d) if !replace => d,
        current => {
            let mut d = initial(records)?;
            d.jd = current.map(|c| c.jd).unwrap_or_default();
            // a chosen 教育 material comes in with the points chosen
            for s in &mut d.sections {
                s.entries.retain(|e| !e.source.as_ref().is_some_and(|src| selections.iter().any(|x| x.record_id == src.record_id)));
            }
            d
        }
    };
    for selection in selections {
        let r = records.iter().find(|r| r.id == selection.record_id && r.kind == "material" && r.enabled).ok_or("所选素材已删除或停用，请重新选材")?;
        if selection.point_ids.iter().any(|id| !r.points.iter().any(|p| &p.id == id)) {
            return Err("素材的要点刚刚改过，请重新选择".into());
        }
        if draft.sections.iter().flat_map(|s| &s.entries).any(|e| e.source.as_ref().is_some_and(|s| s.record_id == r.id)) {
            continue;
        }
        let new_entry = entry(r, &selection.point_ids, id()?);
        match draft.sections.iter_mut().find(|s| s.category == r.category) {
            Some(section) => section.entries.push(new_entry),
            None => draft.sections.push(Section { id: id()?, title: section_title(&r.category), category: r.category.clone(), entries: vec![new_entry] }),
        }
    }
    Ok(draft)
}

#[tauri::command]
pub async fn picky_seeker_assemble(state: State<'_, AppState>, selections: Vec<Selection>, replace: bool) -> Result<Workspace, String> {
    state.with_picky_undo(if replace { "重新组装简历" } else { "增加简历素材" }, |c| {
        let tx = Tx::begin(c)?;
        let view = read(&tx)?;
        let fresh = replace || view.draft.is_none();
        let draft = assemble(&view.catalog.records, view.draft, &selections, replace)?;
        save(&tx, &draft)?;
        if fresh {
            baseline(&tx, &draft)?;
        }
        tx.commit()?;
        read(c)
    })
}

/// Entries whose material has a newer revision: the entry as it would be now (the same points
/// that are still there).
pub fn source_changes(view: &Workspace) -> Vec<SourceChange> {
    let mut changes = vec![];
    for e in view.draft.iter().flat_map(|d| &d.sections).flat_map(|s| &s.entries) {
        let Some(source) = &e.source else { continue };
        let Some(r) = view.catalog.records.iter().find(|r| r.id == source.record_id) else { continue };
        if r.revision != source.revision {
            let chosen: Vec<String> = source.point_ids.iter().filter(|id| r.points.iter().any(|p| &p.id == *id)).cloned().collect();
            changes.push(SourceChange { entry_id: e.id.clone(), title: r.title.clone(), before: e.clone(), after: entry(r, &chosen, e.id.clone()) });
        }
    }
    changes
}

#[tauri::command]
pub async fn picky_seeker_sources(state: State<'_, AppState>) -> Result<Vec<SourceChange>, String> {
    state.picky.with_db(|c| Ok(source_changes(&read(c)?)))
}

#[tauri::command]
pub async fn picky_seeker_source_apply(state: State<'_, AppState>, change: SourceChange) -> Result<Workspace, String> {
    state.with_picky_undo("更新简历来源", |c| {
        let mut draft = read(c)?.draft.ok_or("还没有草稿")?;
        let entry = draft.sections.iter_mut().flat_map(|s| &mut s.entries).find(|e| e.id == change.entry_id).ok_or("这段经历已经不在草稿里")?;
        if *entry != change.before {
            return Err("草稿刚刚改过，请重新看来源差异".into());
        }
        *entry = change.after;
        save(c, &draft)?;
        read(c)
    })
}

#[tauri::command]
pub async fn picky_seeker_confirm(state: State<'_, AppState>, label: String) -> Result<Workspace, String> {
    state.with_picky_undo("确认简历版本", |c| {
        let tx = Tx::begin(c)?;
        let draft = read(&tx)?.draft.ok_or("还没有草稿")?;
        let label = if label.trim().is_empty() { "确认版本" } else { label.trim() };
        tx.execute("INSERT INTO seeker_versions (label, body) VALUES (?1, ?2)", params![label, serde_json::to_string(&draft).map_err(err)?]).map_err(err)?;
        baseline(&tx, &draft)?;
        tx.commit()?;
        read(c)
    })
}

#[tauri::command]
pub async fn picky_seeker_restore(state: State<'_, AppState>, version_id: i64) -> Result<Workspace, String> {
    state.with_picky_undo("从历史另开草稿", |c| {
        let tx = Tx::begin(c)?;
        let body: String = tx.query_row("SELECT body FROM seeker_versions WHERE id = ?1", [version_id], |r| r.get(0)).map_err(|_| "找不到这个版本".to_string())?;
        let draft: Draft = serde_json::from_str(&body).map_err(err)?;
        save(&tx, &draft)?;
        baseline(&tx, &draft)?;
        tx.commit()?;
        read(c)
    })
}

/// `ids` must be exactly one group of siblings in a new order (points stay in their entry).
pub fn reorder(draft: &mut Draft, kind: &str, parent: Option<&str>, ids: &[String]) -> Result<(), String> {
    fn sort<T>(rows: &mut [T], id: impl Fn(&T) -> &str, ids: &[String]) -> Result<(), String> {
        let same = rows.len() == ids.len() && ids.iter().collect::<HashSet<_>>().len() == ids.len() && rows.iter().all(|r| ids.iter().any(|i| i == id(r)));
        if !same {
            return Err("内容刚刚变了，请重新排序".into());
        }
        rows.sort_by_key(|r| ids.iter().position(|i| i == id(r)));
        Ok(())
    }
    match kind {
        "header" => sort(&mut draft.header, |h| &h.id, ids),
        "section" => sort(&mut draft.sections, |s| &s.id, ids),
        "entry" => sort(&mut draft.sections.iter_mut().find(|s| Some(s.id.as_str()) == parent).ok_or("找不到章节")?.entries, |e| &e.id, ids),
        "point" => sort(&mut draft.sections.iter_mut().flat_map(|s| &mut s.entries).find(|e| Some(e.id.as_str()) == parent).ok_or("找不到经历")?.points, |p| &p.id, ids),
        _ => Err("未知的排序类型".into()),
    }
}

#[tauri::command]
pub async fn picky_seeker_reorder(state: State<'_, AppState>, kind: String, parent: Option<String>, ids: Vec<String>) -> Result<Workspace, String> {
    state.with_picky_undo("调整简历顺序", |c| {
        let mut draft = read(c)?.draft.ok_or("还没有草稿")?;
        reorder(&mut draft, &kind, parent.as_deref(), &ids)?;
        save(c, &draft)?;
        read(c)
    })
}

/// A model's suggestion the user accepted: only onto the draft it was made from.
#[tauri::command]
pub async fn picky_seeker_apply(state: State<'_, AppState>, before: Draft, after: Draft) -> Result<Workspace, String> {
    state.with_picky_undo("接受简历建议", |c| {
        if read(c)?.draft.as_ref() != Some(&before) {
            return Err("草稿刚刚改过，请重新生成建议".into());
        }
        save(c, &after)?;
        read(c)
    })
}
