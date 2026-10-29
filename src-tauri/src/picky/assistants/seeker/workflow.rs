//! Assembling the draft from materials, a material's newer version, sorting.
//! 增加 appends the chosen materials not in the draft yet (text, order and header stay); 重新组装 and
//! 新建 start over from 基本信息 + 教育 (+ the chosen ones; 重新组装 keeps the JD, its job and Resume / CV).
//! Each is one undo step. Everything works on one language's draft with that language's materials.
//! English: 新建 also brings the first Summary (category 摘要: one entry whose note is the text; a draft
//! has one, choosing another replaces it); new sections go in the usual order (Summary, Skills,
//! Education, Projects, Experience, Certifications); Chinese ones go last.
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
/// The English template's contact line: 「Singapore · mail · phone · github.com/…」.
pub const HEADER_EN: [&str; 4] = ["Location", "Email", "Phone", "GitHub"];
/// The profile field that is the name, not a header field.
pub const NAMES: [&str; 2] = ["姓名", "Name"];
/// English sections in this order (by material category).
pub const ORDER_EN: [&str; 6] = ["摘要", "技能", "教育", "项目", "工作", "证书"];
/// Single-entry: one Summary per draft.
pub const SUMMARY: &str = "摘要";

pub fn id() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|e| e.to_string())?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Does a JD read like a development job? Then a material's 技术栈 line is chosen by default (the
/// user can still untick it; 选材 decides from the JD itself).
pub fn dev_jd(jd: &str) -> bool {
    let t = jd.to_lowercase();
    ["开发", "研发", "工程师", "程序员", "后端", "前端", "全栈", "developer", "engineer", "software", "programmer", "backend", "back-end", "frontend", "front-end", "full stack", "full-stack"].iter().any(|k| t.contains(k))
}

/// 技术栈 lines: chosen by default only for a development JD (`dev_jd`).
pub fn is_stack(p: &Point) -> bool {
    let t = p.text.trim_start_matches(['*', ' ']).to_lowercase();
    t.starts_with("技术栈") || t.starts_with("tech stack")
}

fn entry(record: &Record, chosen: &[String], entry_id: String) -> Entry {
    let points = record.choosable().into_iter().filter(|p| chosen.contains(&p.id)).map(|p| Point { id: format!("{entry_id}:{}", p.id), text: p.text }).collect();
    let source = Source { record_id: record.id.clone(), revision: record.revision, point_ids: chosen.to_vec() };
    Entry { id: entry_id, time: record.time.clone(), name: record.title.clone(), role: record.role.clone(), location: record.location.clone(), note: record.text.clone(), points, source: Some(source) }
}

fn section_title(category: &str, lang: &str) -> String {
    match (lang, category) {
        ("en", "摘要") => "Profile Summary".into(),
        ("en", "技能") => "Skills".into(),
        ("en", "教育") => "Education".into(),
        ("en", "项目") => "Projects".into(),
        ("en", "工作") => "Experience".into(),
        ("en", "证书") => "Certifications".into(),
        ("en", c) => c.into(),
        (_, "教育") => "教育背景".into(),
        (_, "技能") => "专业技能".into(),
        (_, c) => format!("{c}经历"),
    }
}

fn material(r: &Record, lang: &str) -> bool {
    r.kind == "material" && r.enabled && r.lang == lang
}

pub fn initial(records: &[Record], lang: &str) -> Result<Draft, String> {
    let english = lang == "en";
    let defaults: &[&str] = if english { &HEADER_EN } else { &HEADER };
    let profile = records.iter().find(|r| r.kind == "profile" && r.enabled && r.lang == lang);
    let value = |label: &str| profile.and_then(|p| p.fields.iter().find(|f| f.label == label)).map(|f| f.value.clone()).unwrap_or_default();
    let name = NAMES.iter().map(|n| value(n)).find(|v| !v.is_empty()).unwrap_or_default();
    let mut header: Vec<HeaderField> = defaults.iter().enumerate().map(|(i, label)| HeaderField { id: format!("header-{i}"), label: (*label).into(), value: value(label), visible: true }).collect();
    for f in profile.map(|p| p.fields.as_slice()).unwrap_or_default() {
        if !NAMES.contains(&f.label.as_str()) && !defaults.contains(&f.label.as_str()) {
            header.push(HeaderField { id: format!("header-{}", header.len()), label: f.label.clone(), value: f.value.clone(), visible: false });
        }
    }
    let all = |r: &Record| r.choosable().into_iter().map(|p| p.id).collect::<Vec<_>>();
    let mut sections = vec![];
    if let Some(summary) = records.iter().find(|r| english && material(r, lang) && r.category == SUMMARY) {
        sections.push(Section { id: id()?, title: section_title(SUMMARY, lang), category: SUMMARY.into(), entries: vec![entry(summary, &all(summary), id()?)] });
    }
    let education = records.iter().filter(|r| material(r, lang) && r.category == "教育").map(|r| Ok(entry(r, &all(r), id()?))).collect::<Result<_, String>>()?;
    sections.push(Section { id: id()?, title: section_title("教育", lang), category: "教育".into(), entries: education });
    Ok(Draft { lang: lang.into(), form: if english { "resume".into() } else { String::new() }, name, header, sections, jd: String::new(), job: None })
}

pub fn baseline(c: &Connection, draft: &Draft) -> Result<(), String> {
    c.execute("UPDATE seeker_draft SET baseline = ?1 WHERE lang = ?2", params![serde_json::to_string(draft).map_err(err)?, draft.lang]).map_err(err)?;
    Ok(())
}

/// Where a new section of `category` goes: English in the usual order, Chinese last.
fn section_at(draft: &Draft, category: &str) -> usize {
    let rank = |c: &str| ORDER_EN.iter().position(|x| *x == c).unwrap_or(ORDER_EN.len());
    if draft.lang != "en" {
        return draft.sections.len();
    }
    draft.sections.iter().position(|s| rank(&s.category) > rank(category)).unwrap_or(draft.sections.len())
}

/// The draft (of `lang`) with the chosen materials added (replace: a new draft keeping the JD, its
/// job and Resume / CV).
pub fn assemble(records: &[Record], current: Option<Draft>, selections: &[Selection], replace: bool, lang: &str) -> Result<Draft, String> {
    let chosen = |r: &Record| selections.iter().any(|x| x.record_id == r.id);
    if records.iter().filter(|r| r.category == SUMMARY && chosen(r)).count() > 1 {
        return Err("Summary 只能选一条".into());
    }
    let mut draft = match current {
        Some(d) if !replace => d,
        current => {
            let mut d = initial(records, lang)?;
            if let Some(c) = current {
                (d.jd, d.job) = (c.jd, c.job);
                if lang == "en" {
                    d.form = c.form;
                }
            }
            // a chosen 教育 material comes in with the points chosen
            for s in &mut d.sections {
                s.entries.retain(|e| !e.source.as_ref().is_some_and(|src| selections.iter().any(|x| x.record_id == src.record_id)));
            }
            d
        }
    };
    for selection in selections {
        let r = records.iter().find(|r| r.id == selection.record_id && material(r, lang)).ok_or("所选素材已删除或停用，请重新选材")?;
        if selection.point_ids.iter().any(|id| !r.choosable().iter().any(|p| &p.id == id)) {
            return Err("素材的要点刚刚改过，请重新选择".into());
        }
        if draft.sections.iter().flat_map(|s| &s.entries).any(|e| e.source.as_ref().is_some_and(|s| s.record_id == r.id)) {
            continue;
        }
        let new_entry = entry(r, &selection.point_ids, id()?);
        match draft.sections.iter_mut().find(|s| s.category == r.category) {
            Some(section) if r.category == SUMMARY => section.entries = vec![new_entry],
            Some(section) => section.entries.push(new_entry),
            None => {
                let at = section_at(&draft, &r.category);
                draft.sections.insert(at, Section { id: id()?, title: section_title(&r.category, lang), category: r.category.clone(), entries: vec![new_entry] });
            }
        }
    }
    Ok(draft)
}

#[tauri::command]
pub async fn picky_seeker_assemble(state: State<'_, AppState>, selections: Vec<Selection>, replace: bool, lang: String) -> Result<Workspace, String> {
    let lang = lang_of(&lang)?;
    state.with_picky_undo(if replace { "重新组装简历" } else { "增加简历素材" }, |c| {
        let tx = Tx::begin(c)?;
        let view = read(&tx)?;
        let current = view.draft(lang).cloned();
        let fresh = replace || current.is_none();
        let draft = assemble(&view.catalog.records, current, &selections, replace, lang)?;
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
pub fn source_changes(view: &Workspace, lang: &str) -> Vec<SourceChange> {
    let mut changes = vec![];
    for e in view.draft(lang).iter().flat_map(|d| &d.sections).flat_map(|s| &s.entries) {
        let Some(source) = &e.source else { continue };
        let Some(r) = view.catalog.records.iter().find(|r| r.id == source.record_id) else { continue };
        if r.revision != source.revision {
            let chosen: Vec<String> = source.point_ids.iter().filter(|id| r.choosable().iter().any(|p| &p.id == *id)).cloned().collect();
            changes.push(SourceChange { entry_id: e.id.clone(), title: r.title.clone(), before: e.clone(), after: entry(r, &chosen, e.id.clone()) });
        }
    }
    changes
}

#[tauri::command]
pub async fn picky_seeker_sources(state: State<'_, AppState>, lang: String) -> Result<Vec<SourceChange>, String> {
    let lang = lang_of(&lang)?;
    state.picky.with_db(|c| Ok(source_changes(&read(c)?, lang)))
}

#[tauri::command]
pub async fn picky_seeker_source_apply(state: State<'_, AppState>, change: SourceChange, lang: String) -> Result<Workspace, String> {
    let lang = lang_of(&lang)?;
    state.with_picky_undo("更新简历来源", |c| {
        let mut draft = read(c)?.draft(lang).cloned().ok_or("还没有草稿")?;
        let entry = draft.sections.iter_mut().flat_map(|s| &mut s.entries).find(|e| e.id == change.entry_id).ok_or("这段经历已经不在草稿里")?;
        if *entry != change.before {
            return Err("草稿刚刚改过，请重新看来源差异".into());
        }
        *entry = change.after;
        save(c, &draft)?;
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
pub async fn picky_seeker_reorder(state: State<'_, AppState>, kind: String, parent: Option<String>, ids: Vec<String>, lang: String) -> Result<Workspace, String> {
    let lang = lang_of(&lang)?;
    state.with_picky_undo("调整简历顺序", |c| {
        let mut draft = read(c)?.draft(lang).cloned().ok_or("还没有草稿")?;
        reorder(&mut draft, &kind, parent.as_deref(), &ids)?;
        save(c, &draft)?;
        read(c)
    })
}

/// A model's suggestion the user accepted: only onto the draft it was made from.
#[tauri::command]
pub async fn picky_seeker_apply(state: State<'_, AppState>, before: Draft, after: Draft) -> Result<Workspace, String> {
    state.with_picky_undo("接受简历建议", |c| {
        if after.lang != before.lang || read(c)?.draft(&before.lang) != Some(&before) {
            return Err("草稿刚刚改过，请重新生成建议".into());
        }
        save(c, &after)?;
        read(c)
    })
}
