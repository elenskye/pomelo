//! Atlas's 自建模块的一幕 (v1.0 Phase 7, Tabloid 1.1.0): a made module (五花八门 / 循序渐进) with something in the year
//! gets a page of its own. The program doesn't fix what it looks like (站主 11-14 问卷, the canvas approved 11-16):
//! 1. Rust reads the year (data.rs `year_made`, the same reading 年度总结 gets).
//! 2. 柚子 lays each one out by `SKILL` (one call after the scenes, `MARK`): skip it, or a layout and a part in each
//!    of its slots (a field, a quote by its number), and a line or two. Words only: every number is filled in here.
//! 3. Rust checks the layout against `LAYOUTS` / `FITS` and the module's fields and fills each part's numbers
//!    (`fill`); what's wrong goes back to 柚子 (twice at most), after that the module gets `default_plan`.
//! 4. The design draws `Plan` as given; a run whose scenes didn't get written draws `MadeYear.plan` (the default).
//! The sticker on the page: the one picked for the module (`ui.module_look`), else its icon's (`MADE_MARKS`).

use super::data::{self, md, MadeField, YearMade};
use super::provider::{Msg, Target};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::HashMap;

/// Starts the layout step's system message (the mock model answers by it).
pub const MARK: &str = "【Atlas · 自建模块】";

/// The layouts and their slots (the design draws them: H a wide block, A / B halves, S a strip, Q a slip).
pub const LAYOUTS: &[(&str, &[&str])] = &[("hero", &["H", "S", "Q"]), ("pair", &["A", "B", "S", "Q"]), ("list", &["H", "A", "B"])];
/// Each part and the slots it fits.
pub const FITS: &[(&str, &str)] =
    &[("big", "HAB"), ("compare", "HAB"), ("stars", "HAB"), ("dist", "HAB"), ("chips", "HAB"), ("sum", "HAB"), ("courses", "HA"), ("months", "S"), ("cards", "QH"), ("quote", "Q")];

/// A made module's icon → its sticker (src/modules/custom/icons.ts `MADE_MARKS`, visual-atelier marks/modules/).
const MADE_MARKS: &[(&str, &str)] = &[
    ("shapes", "shapes"), ("list-checks", "checklist"), ("gamepad-2", "game"), ("music", "music"), ("headphones", "headphones"), ("camera", "camera"),
    ("tv", "tv"), ("coffee", "coffee"), ("utensils", "bowl"), ("shopping-bag", "bag"), ("gift", "gift"), ("shirt", "shirt"), ("plane", "plane"),
    ("mountain", "mountain"), ("leaf", "leaf"), ("sprout", "sprout"), ("dumbbell", "dumbbell"), ("heart", "heart"), ("star", "star"),
    ("graduation-cap", "grad"), ("languages", "translate"), ("code", "code"), ("puzzle", "puzzle"), ("trophy", "trophy"),
];
const LOOK_KEY: &str = "ui.module_look";

/// Lines per page and characters per line (as the scenes').
const LINES: usize = 2;
const LINE_CHARS: usize = 80;
/// The reply's room: a little for each module.
const BASE_TOKENS: i64 = 800;
const MODULE_TOKENS: i64 = 500;
/// Asked again this many times for the modules whose layout didn't fit.
const AGAIN: usize = 2;
/// Items of a module the model sees.
const ITEM_LINES: usize = 30;

/// A filled part: where it goes, what it is, and what it draws (numbers, names, keys).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Part {
    pub slot: String,
    pub part: String,
    pub data: Value,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Plan {
    pub layout: String,
    pub parts: Vec<Part>,
}

/// A made module in the snapshot: its year, its sticker and the layout it gets without 柚子.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MadeYear {
    #[serde(flatten)]
    pub year: YearMade,
    pub mark: String,
    pub plan: Plan,
}

/// The modules for the snapshot, each with its sticker and default layout.
pub fn made_years(pomelo: &rusqlite::Connection, access: &data::Access, year: &str, from: &str, to: &str) -> Result<Vec<MadeYear>, String> {
    let looks = crate::settings::get(pomelo, LOOK_KEY).ok().flatten().unwrap_or(Value::Null);
    Ok(data::year_made(pomelo, access, year, from, to)?
        .into_iter()
        .map(|y| {
            // a picked sticker the design carries (it bundles the made modules' ones: Tabloid parts.mjs MADE_MARKS)
            let picked = looks.get(&y.key).and_then(|l| l.get("mark")).and_then(Value::as_str).filter(|m| MADE_MARKS.iter().any(|(_, x)| x == m)).map(String::from);
            let mark = picked.unwrap_or_else(|| MADE_MARKS.iter().find(|(i, _)| *i == y.icon).map_or("shapes", |(_, m)| m).to_string());
            let plan = default_plan(&y);
            MadeYear { year: y, mark, plan }
        })
        .collect())
}

/// One slot as 柚子 writes it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Spec {
    pub part: String,
    pub field: Option<String>,
    pub quote: Option<String>,
    pub label: Option<String>,
    pub unit: Option<String>,
}

impl Spec {
    fn of(part: &str) -> Self {
        Spec { part: part.into(), ..Default::default() }
    }
    fn field(part: &str, f: &str) -> Self {
        Spec { part: part.into(), field: Some(f.into()), ..Default::default() }
    }
}

fn kind_name(k: &str) -> &'static str {
    match k {
        "number" => "数字",
        "rating" => "评分",
        "select" => "单选",
        "multi" => "多选",
        "longtext" => "长文字",
        "year" => "年份",
        "check" => "勾选",
        "link" => "链接",
        _ => "文字",
    }
}

fn short(s: &Option<String>, n: usize) -> Option<String> {
    s.as_deref().map(str::trim).filter(|t| !t.is_empty()).map(|t| t.chars().take(n).collect())
}

fn the_field<'a>(y: &'a YearMade, spec: &Spec, kinds: &[&str]) -> Result<&'a MadeField, String> {
    let name = short(&spec.field, 40).ok_or_else(|| format!("{} 要写 field（字段名）", spec.part))?;
    let f = y.fields.iter().find(|f| f.name == name).ok_or_else(|| format!("没有叫「{name}」的字段"))?;
    if !kinds.contains(&f.kind.as_str()) {
        return Err(format!("{} 要{}字段，「{name}」是{}", spec.part, kinds.iter().map(|k| kind_name(k)).collect::<Vec<_>>().join("或"), kind_name(&f.kind)));
    }
    Ok(f)
}

fn num(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

/// 1234.0 → 1234; 3.5 → 3.5 (as JSON numbers).
fn number(x: f64) -> Value {
    if x.fract() == 0.0 && x.abs() < 1e15 { json!(x as i64) } else { json!((x * 100.0).round() / 100.0) }
}

/// How often each option of a 单选 / 多选 field comes up this year, most first.
fn tally(y: &YearMade, f: &MadeField) -> Vec<(String, i64)> {
    let mut n: Vec<(String, i64)> = vec![];
    for it in &y.items {
        let words: Vec<String> = match it.values.get(&f.name) {
            Some(Value::String(s)) => vec![s.clone()],
            Some(Value::Array(xs)) => xs.iter().filter_map(Value::as_str).map(String::from).collect(),
            _ => vec![],
        };
        for w in words {
            match n.iter_mut().find(|(x, _)| *x == w) {
                Some(e) => e.1 += 1,
                None => n.push((w, 1)),
            }
        }
    }
    n.sort_by_key(|x| std::cmp::Reverse(x.1));
    n
}

/// A slot's part with its numbers filled in; Err: what's wrong, said to 柚子.
pub fn fill(y: &YearMade, spec: &Spec) -> Result<Value, String> {
    let list = y.template == "list";
    let unit = || short(&spec.unit, 3).unwrap_or_else(|| if list { "条".into() } else { y.lesson_word.clone() });
    let label = |d: &str| short(&spec.label, 10).unwrap_or_else(|| d.to_string());
    Ok(match spec.part.as_str() {
        "big" => match &spec.field {
            Some(_) => {
                let f = the_field(y, spec, &["number"])?;
                let total: f64 = y.items.iter().filter_map(|it| it.values.get(&f.name).and_then(num)).sum();
                json!({ "label": label(&f.name), "n": number(total), "unit": short(&spec.unit, 3).unwrap_or_default() })
            }
            None => json!({ "label": label(if list { "这一年记了" } else { "这一年学完" }), "n": y.n, "unit": unit(), "last": (y.last > 0).then_some(y.last) }),
        },
        "compare" => {
            if y.last == 0 {
                return Err("去年没有数，不能用 compare".into());
            }
            json!({ "label": label("和去年比"), "n": y.n, "last": y.last, "unit": unit() })
        }
        "stars" => {
            let f = the_field(y, spec, &["rating"])?;
            let mut rated: Vec<(&data::MadeItem, i64)> = y.items.iter().filter_map(|it| it.values.get(&f.name).and_then(Value::as_i64).filter(|r| *r > 0).map(|r| (it, r))).collect();
            if rated.is_empty() {
                return Err(format!("这一年没有填「{}」的", f.name));
            }
            rated.sort_by_key(|x| std::cmp::Reverse(x.1));
            json!({ "label": label("最喜欢的"), "items": rated.iter().take(3).map(|(it, r)| json!({ "key": it.key, "title": it.title, "rating": r })).collect::<Vec<_>>() })
        }
        "dist" | "chips" => {
            let f = the_field(y, spec, if spec.part == "dist" { &["select", "multi"] } else { &["multi", "select"] })?;
            let rows = tally(y, f);
            if rows.is_empty() {
                return Err(format!("这一年没有填「{}」的", f.name));
            }
            json!({ "label": label(&f.name), "rows": rows.iter().take(if spec.part == "dist" { 6 } else { 8 }).map(|(name, n)| json!({ "name": name, "n": n })).collect::<Vec<_>>() })
        }
        "sum" => {
            let f = the_field(y, spec, &["number"])?;
            let mut vals: Vec<(&str, f64)> = y.items.iter().filter_map(|it| it.values.get(&f.name).and_then(num).map(|v| (it.title.as_str(), v))).collect();
            if vals.is_empty() {
                return Err(format!("这一年没有填「{}」的", f.name));
            }
            vals.sort_by(|a, b| b.1.total_cmp(&a.1));
            let total: f64 = vals.iter().map(|x| x.1).sum();
            let mut rows: Vec<Value> = vals.iter().take(3).map(|(t, v)| json!([t, number(*v)])).collect();
            if vals.len() > 3 {
                rows.push(json!([format!("还有 {} 条", vals.len() - 3), number(vals[3..].iter().map(|x| x.1).sum())]));
            }
            json!({ "label": label(&f.name), "rows": rows, "total": number(total), "unit": short(&spec.unit, 3).unwrap_or_default() })
        }
        "courses" => {
            if list {
                return Err("courses 只给循序渐进".into());
            }
            json!({ "label": label(&format!("这一年学的{}", y.course_word)), "unit": y.lesson_word, "items": y.courses.iter().take(4).collect::<Vec<_>>() })
        }
        "months" => json!({ "label": label(if list { "每个月记的" } else { "每个月学完的" }), "values": y.months, "unit": unit() }),
        "cards" => {
            if !list {
                return Err("cards 只给五花八门".into());
            }
            // the best rated first (a 评分 field), else the newest
            let rating = y.fields.iter().find(|f| f.kind == "rating");
            let mut items: Vec<&data::MadeItem> = y.items.iter().collect();
            match rating {
                Some(f) => items.sort_by_key(|it| std::cmp::Reverse(it.values.get(&f.name).and_then(Value::as_i64).unwrap_or(0))),
                None => items.sort_by(|a, b| b.day.cmp(&a.day)),
            }
            let tag = y.fields.iter().find(|f| f.kind == "select" || f.kind == "text");
            let long = y.fields.iter().find(|f| f.kind == "longtext");
            let cards: Vec<Value> = items
                .iter()
                .take(4)
                .map(|it| {
                    let first = [tag.and_then(|f| it.values.get(&f.name)).and_then(Value::as_str).map(String::from), it.day.as_deref().map(md)].into_iter().flatten().collect::<Vec<_>>().join(" · ");
                    let words = long.and_then(|f| it.values.get(&f.name)).and_then(Value::as_str).map(|t| format!("「{}」", t.chars().take(20).collect::<String>()));
                    let sub: Vec<String> = [Some(first).filter(|x| !x.is_empty()), words].into_iter().flatten().collect();
                    let r = rating.and_then(|f| it.values.get(&f.name)).and_then(Value::as_i64);
                    json!({ "key": it.key, "title": it.title, "sub": sub, "rating": r })
                })
                .collect();
            json!({ "items": cards })
        }
        "quote" => {
            let q = short(&spec.quote, 10).ok_or("quote 要写 ref（能引用的话的编号，像 q1）")?;
            let i: usize = q.trim_start_matches(['q', 'Q']).parse().map_err(|_| format!("没有编号是 {q} 的话"))?;
            let hit = i.checked_sub(1).and_then(|i| y.quotes.get(i)).ok_or_else(|| format!("没有编号是 {q} 的话"))?;
            json!({ "key": hit.key, "text": hit.text, "from": hit.from })
        }
        other => return Err(format!("没有叫 {other} 的零件")),
    })
}

/// A layout checked and filled; Err: everything wrong with it.
pub fn plan_of(y: &YearMade, layout: &str, slots: &[(String, Spec)]) -> Result<Plan, Vec<String>> {
    let mut bad = vec![];
    let Some((_, allowed)) = LAYOUTS.iter().find(|(k, _)| *k == layout) else {
        return Err(vec![format!("没有叫 {layout} 的排法（只有 hero、pair、list）")]);
    };
    let mut parts = vec![];
    for (slot, spec) in slots {
        if !allowed.contains(&slot.as_str()) {
            bad.push(format!("{layout} 没有 {slot} 这个位置（只有 {}）", allowed.join("、")));
            continue;
        }
        if parts.iter().any(|p: &Part| p.slot == *slot) {
            bad.push(format!("{slot} 放了两个零件"));
            continue;
        }
        match FITS.iter().find(|(p, _)| *p == spec.part) {
            None => bad.push(format!("没有叫 {} 的零件", spec.part)),
            Some((_, fits)) if !fits.contains(slot.as_str()) => bad.push(format!("{} 不能放在 {slot}（只能放 {}）", spec.part, fits.chars().map(String::from).collect::<Vec<_>>().join("、"))),
            Some(_) => match fill(y, spec) {
                Ok(data) => parts.push(Part { slot: slot.clone(), part: spec.part.clone(), data }),
                Err(e) => bad.push(format!("{slot} 的 {}：{e}", spec.part)),
            },
        }
    }
    if parts.is_empty() && bad.is_empty() {
        bad.push("一个零件都没放".into());
    }
    if bad.is_empty() { Ok(Plan { layout: layout.into(), parts }) } else { Err(bad) }
}

/// The layout a module gets without 柚子 (its step failed, or what it gave never fit): 循序渐进 the courses on top;
/// 五花八门 the count beside the field with the most to show, the months, a quote (or the best few); few items
/// (≤ 3) one slip each.
pub fn default_plan(y: &YearMade) -> Plan {
    let has = |k: &str| y.fields.iter().find(|f| f.kind == k).map(|f| f.name.clone());
    let quote = (!y.quotes.is_empty()).then(|| Spec { part: "quote".into(), quote: Some("q1".into()), ..Default::default() });
    let (layout, slots): (&str, Vec<(&str, Option<Spec>)>) = if y.template == "progress" {
        ("hero", vec![("H", Some(Spec::of("courses"))), ("S", Some(Spec::of("months"))), ("Q", quote)])
    } else {
        let side = has("rating").map(|f| Spec::field("stars", &f)).or_else(|| has("select").or_else(|| has("multi")).map(|f| Spec::field("dist", &f))).or_else(|| has("number").map(|f| Spec::field("sum", &f))).or_else(|| (y.last > 0).then(|| Spec::of("compare")));
        if y.n <= 3 {
            ("list", vec![("H", Some(Spec::of("cards"))), ("A", Some(Spec::of("big"))), ("B", side)])
        } else {
            ("pair", vec![("A", Some(Spec::of("big"))), ("B", side), ("S", Some(Spec::of("months"))), ("Q", quote.or_else(|| Some(Spec::of("cards"))))])
        }
    };
    // a part that can't be filled is left out (an empty slot is fine)
    let parts = slots.into_iter().filter_map(|(slot, spec)| spec.and_then(|s| fill(y, &s).ok().map(|data| Part { slot: slot.into(), part: s.part, data }))).collect();
    Plan { layout: layout.into(), parts }
}

// ---------- 柚子 lays them out ----------

pub const SKILL: &str = "# 年度小报 · 自建模块的一幕

用户在柚儿园里自己建的模块（五花八门是清单，循序渐进是一门门课），这一年有数据的每个都给你。每个模块回一份排法，觉得没意思的回 skip。

## 排法
- layout：hero（一个主角：H 宽的大块、S 一条、Q 一张纸条）、pair（左右两半：A、B、S、Q）、list（条目少、每条都值得看：H 高的一大块、A、B）
- slots：每个位置放一个零件，用不着的位置可以空着
- lines：1～2 句话，每句不超过 40 个字，说这一年这个模块里最有意思的事，对着「你」说；不写数字（页上有）

## 零件（括号里是能放的位置和要写的东西）
- big 一年几条（H A B）；写 field（数字字段）就是那个字段的合计
- compare 和去年比（H A B；去年有数才用）
- stars 评分最高的几条（H A B；field 写评分字段）
- dist 单选、多选字段的分布（H A B；field）
- chips 多选字段的词，一个词一张圆贴纸（H A B；field）
- sum 数字字段的合计，打成小票（H A B；field）
- courses 循序渐进：这一年学的几门（H A）
- months 十二个月一排小条（只放 S）
- cards 几条条目贴在纸上（Q H；五花八门）
- quote 一句原话（只放 Q；ref 写能引用的话的编号，像 q1）
big、compare、sum、months 可以加 label（不超过 10 个字，像「这一年新去了」）和 unit（不超过 3 个字，像「家」「公里」），说得像这个模块；不加用默认的。

## 怎么挑
1. 循序渐进先用 courses；清单有评分字段先想 stars。
2. 条目少于 4 条用 list 排法，H 放 cards。
3. 有数字字段且加起来有意思（公里、页数、花的钱）用 sum。
4. 有能引用的话就挑一句最打动人的放 Q；没有就不放 quote，Q 换成 cards。
5. 不写数字，不编条目：数字程序算，名字照数据里的写。字段名照给的写。";

fn field_line(y: &YearMade, f: &MadeField) -> String {
    let more = match f.kind.as_str() {
        "select" | "multi" => {
            let t = tally(y, f);
            if t.is_empty() { String::new() } else { format!("：{}", t.iter().take(8).map(|(n, c)| format!("{n} {c}")).collect::<Vec<_>>().join("、")) }
        }
        "number" => {
            let vals: Vec<f64> = y.items.iter().filter_map(|it| it.values.get(&f.name).and_then(num)).collect();
            if vals.is_empty() { String::new() } else { format!("，{} 条填了，合计 {}", vals.len(), number(vals.iter().sum())) }
        }
        "rating" => format!("，{} 条打了分", y.items.iter().filter(|it| it.values.get(&f.name).and_then(Value::as_i64).is_some_and(|r| r > 0)).count()),
        _ => String::new(),
    };
    format!("{}（{}{more}）", f.name, kind_name(&f.kind))
}

/// What 柚子 is told about one module.
fn module_text(y: &YearMade) -> String {
    let list = y.template == "list";
    let unit = if list { "条" } else { y.lesson_word.as_str() };
    let mut out = vec![
        format!("## {} {}（{}）", y.key, y.name, if list { "五花八门" } else { "循序渐进" }),
        format!("这一年 {} {unit}，去年 {} {unit}；每个月：{}", y.n, y.last, y.months.iter().map(i64::to_string).collect::<Vec<_>>().join(" ")),
    ];
    if list {
        if !y.fields.is_empty() {
            out.push(format!("字段：{}", y.fields.iter().map(|f| field_line(y, f)).collect::<Vec<_>>().join("、")));
        }
        out.push(format!(
            "这一年的条目{}：\n{}",
            if y.items.len() > ITEM_LINES { format!("（前 {ITEM_LINES} 条）") } else { String::new() },
            y.items
                .iter()
                .take(ITEM_LINES)
                .map(|it| {
                    let bits: Vec<String> = y.fields.iter().filter(|f| f.kind != "longtext").filter_map(|f| it.values.get(&f.name).map(|v| (f, v))).map(|(f, v)| format!("{} {}", f.name, match v { Value::Array(xs) => xs.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("、"), Value::String(s) => s.chars().take(30).collect(), v => v.to_string() })).collect();
                    format!("- {}{}", it.title, if bits.is_empty() { String::new() } else { format!("：{}", bits.join("，")) })
                })
                .collect::<Vec<_>>()
                .join("\n")
        ));
    } else {
        out.push(format!("这一年学的{}：\n{}", y.course_word, y.courses.iter().map(|c| format!("- {}：这一年 {} {unit}，一共 {} / {}{}", c.name, c.year, c.done, c.total, if c.finished.is_some() { "，学完了" } else { "" })).collect::<Vec<_>>().join("\n")));
    }
    if !y.quotes.is_empty() {
        out.push(format!("能引用的话：\n{}", y.quotes.iter().enumerate().map(|(i, q)| format!("- q{} 「{}」（{}）", i + 1, q.text, q.from)).collect::<Vec<_>>().join("\n")));
    }
    out.join("\n")
}

fn messages(made: &[&MadeYear], letter: &str) -> Vec<Msg> {
    let system = format!(
        "{MARK}你是柚子，用户自己的生活记录 App「柚儿园」里的助手。你刚写完这一年的年度总结（在最后），Atlas 把它做成一本年度小报，自建模块一个模块一页。照下面的 SKILL 给每个模块排这一页。\n\n{SKILL}\n\n只输出一个 JSON 对象，从 {{ 开始、到 }} 结束，不要代码块、不要别的文字；话里引用原话用「」，不要用英文双引号：{{\"modules\":[{{\"key\":\"list/3\",\"layout\":\"pair\",\"slots\":{{\"A\":{{\"part\":\"big\",\"label\":\"这一年新去了\",\"unit\":\"家\"}},\"B\":{{\"part\":\"dist\",\"field\":\"区域\"}},\"S\":{{\"part\":\"months\"}},\"Q\":{{\"part\":\"quote\",\"ref\":\"q2\"}}}},\"lines\":[\"……\"]}},{{\"key\":\"progress/2\",\"skip\":true}}]}}"
    );
    let user = format!("【自建模块】\n\n{}\n\n【刚写的信】\n{letter}", made.iter().map(|m| module_text(&m.year)).collect::<Vec<_>>().join("\n\n"));
    vec![Msg::new("system", &system), Msg::new("user", &user)]
}

/// What 柚子 wrote for one module: skip, or a layout.
#[derive(Debug, Clone, PartialEq)]
pub enum Raw {
    Skip,
    Laid { layout: String, slots: Vec<(String, Spec)>, lines: Vec<String> },
}

fn text(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn raw_one(o: &Map<String, Value>) -> Option<(String, Raw)> {
    let key = o.get("key").and_then(Value::as_str)?.trim().to_string();
    if o.get("skip").and_then(Value::as_bool) == Some(true) || o.get("layout").and_then(Value::as_str) == Some("skip") {
        return Some((key, Raw::Skip));
    }
    let layout = o.get("layout").and_then(Value::as_str).unwrap_or_default().trim().to_lowercase();
    let slots = match o.get("slots") {
        Some(Value::Object(m)) => m
            .iter()
            .map(|(slot, v)| {
                let spec = match v {
                    Value::String(p) => Spec::of(p.trim()),
                    Value::Object(s) => Spec {
                        part: s.get("part").and_then(Value::as_str).unwrap_or_default().trim().to_lowercase(),
                        field: text(s.get("field")),
                        quote: text(s.get("ref").or_else(|| s.get("quote"))),
                        label: text(s.get("label")),
                        unit: text(s.get("unit")),
                    },
                    _ => Spec::default(),
                };
                (slot.trim().to_uppercase(), spec)
            })
            .collect(),
        _ => vec![],
    };
    let lines = match o.get("lines").or_else(|| o.get("line")) {
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).map(String::from).collect(),
        Some(Value::String(s)) => vec![s.clone()],
        _ => vec![],
    };
    Some((key, Raw::Laid { layout, slots, lines }))
}

/// The modules in a reply (`{"modules": [...]}`, or the list alone); a reply that isn't JSON holds none.
pub fn raw_modules(reply: &str) -> Vec<(String, Raw)> {
    for (i, _) in reply.char_indices().filter(|(_, c)| *c == '{' || *c == '[').take(40) {
        let v = match serde_json::Deserializer::from_str(&reply[i..]).into_iter::<Value>().next() {
            Some(Ok(v)) => v,
            _ => continue,
        };
        let list = match &v {
            Value::Object(o) => o.get("modules").and_then(Value::as_array).cloned().unwrap_or_default(),
            Value::Array(a) => a.clone(),
            _ => vec![],
        };
        let found: Vec<(String, Raw)> = list.iter().filter_map(Value::as_object).filter_map(raw_one).collect();
        if !found.is_empty() {
            return found;
        }
    }
    vec![]
}

/// The page of a module: its key, 柚子's lines and its layout (None: skipped).
pub type Laid = (String, Option<(Vec<String>, Plan)>);

/// Reads one reply for `wanted`: the ones that came out right (`Laid`), and what's wrong with each of the others.
pub fn read(reply: &str, wanted: &[&MadeYear]) -> (Vec<Laid>, Vec<(String, String)>) {
    let raw = raw_modules(reply);
    let (mut ok, mut bad) = (vec![], vec![]);
    for m in wanted {
        let y = &m.year;
        match raw.iter().find(|(k, _)| *k == y.key).map(|x| &x.1) {
            None => bad.push((y.key.clone(), "这一次没有它".into())),
            Some(Raw::Skip) => ok.push((y.key.clone(), None)),
            Some(Raw::Laid { layout, slots, lines }) => match plan_of(y, layout, slots) {
                Ok(plan) => {
                    let lines = lines.iter().map(|l| l.trim()).filter(|l| !l.is_empty()).map(|l| if l.chars().count() > LINE_CHARS { format!("{}…", l.chars().take(LINE_CHARS).collect::<String>()) } else { l.to_string() }).take(LINES).collect();
                    ok.push((y.key.clone(), Some((lines, plan))));
                }
                Err(e) => bad.push((y.key.clone(), e.join("；"))),
            },
        }
    }
    (ok, bad)
}

/// 柚子 lays the modules out: asked once, then again (`AGAIN` times) for the ones whose layout didn't fit, told
/// what's wrong; the ones still wrong (or all, when the model fails) get their default layout, without lines.
/// Also what each answer used (for the run's count).
pub async fn arrange(target: &Target, thinking: bool, made: &[MadeYear], letter: &str) -> (Vec<Laid>, Vec<Option<(i64, i64)>>) {
    let mut used = vec![];
    let mut left: Vec<&MadeYear> = made.iter().collect();
    let mut done: HashMap<String, Option<(Vec<String>, Plan)>> = HashMap::new();
    let mut convo = messages(&left, letter);
    for attempt in 0..=AGAIN {
        if left.is_empty() {
            break;
        }
        let room = BASE_TOKENS + MODULE_TOKENS * left.len() as i64;
        let think = thinking && attempt == 0;
        let room = if think { room + super::context::think_room(32_768) } else { room };
        let mut out = String::new();
        let mut usage = None;
        let streamed = super::provider::stream(
            target.clone(),
            convo.clone(),
            super::provider::Options { max_tokens: Some(room), thinking: Some(think), effort: (attempt > 0).then(|| "low".to_string()), room_fixed: true, ..Default::default() },
            |e| match e {
                super::provider::Event::Content(t) => out.push_str(&t),
                super::provider::Event::Usage { prompt, completion } => usage = Some((prompt, completion)),
                _ => {}
            },
        );
        match tokio::time::timeout(std::time::Duration::from_secs(300), streamed).await {
            Ok(Ok(_)) => used.push(usage),
            _ => break,
        }
        let (ok, bad) = read(&out, &left);
        for (k, v) in ok {
            done.insert(k, v);
        }
        if bad.is_empty() {
            break;
        }
        eprintln!("Atlas made modules, try {}: {}", attempt + 1, bad.iter().map(|(k, e)| format!("{k}: {e}")).collect::<Vec<_>>().join(" | "));
        left.retain(|m| bad.iter().any(|(k, _)| *k == m.year.key));
        convo.push(Msg::new("assistant", &out));
        convo.push(Msg::new(
            "user",
            &format!("这几个模块的排法不对：\n{}\n只为这几个模块重写，同样的 JSON。", bad.iter().map(|(k, e)| format!("- {k}：{e}")).collect::<Vec<_>>().join("\n")),
        ));
    }
    (made.iter().map(|m| (m.year.key.clone(), done.remove(&m.year.key).unwrap_or_else(|| Some((vec![], m.plan.clone()))))).collect(), used)
}

/// The mock model's layout: each module asked for, laid out its default way (by key from the 【自建模块】 list),
/// one skipped when there are three or more; asked again, it does the same.
pub fn mock_reply(user: &str) -> String {
    let keys: Vec<&str> = user.lines().filter_map(|l| l.strip_prefix("## ")?.split(' ').next()).collect();
    let mods: Vec<Value> = keys
        .iter()
        .enumerate()
        .map(|(i, k)| {
            if i == 2 {
                return json!({ "key": k, "skip": true });
            }
            if k.starts_with("progress/") {
                json!({ "key": k, "layout": "hero", "slots": { "H": { "part": "courses" }, "S": { "part": "months" } }, "lines": [format!("（模拟）「{k}」这一年学得很稳。")] })
            } else {
                json!({ "key": k, "layout": "pair", "slots": { "A": { "part": "big", "label": "这一年加了" }, "S": { "part": "months" } }, "lines": [format!("（模拟）「{k}」这一年的话。")] })
            }
        })
        .collect();
    json!({ "modules": mods }).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use data::{MadeItem, MadeQuote};

    fn cafes() -> YearMade {
        let item = |id: i64, title: &str, area: &str, rating: i64, price: f64, words: &[&str], day: &str| MadeItem {
            key: format!("list/3:{id}"),
            title: title.into(),
            day: Some(day.into()),
            values: serde_json::from_value(json!({ "区域": area, "评分": rating, "价格": price, "招牌": words })).unwrap(),
        };
        YearMade {
            key: "list/3".into(),
            name: "咖啡馆".into(),
            template: "list".into(),
            icon: "coffee".into(),
            fields: [("区域", "select"), ("评分", "rating"), ("价格", "number"), ("招牌", "multi"), ("备注", "longtext")].iter().map(|(n, k)| MadeField { name: (*n).into(), kind: (*k).into() }).collect(),
            items: vec![
                item(1, "江边的小窗", "江边", 5, 32.0, &["手冲", "可颂"], "2026-03-02"),
                item(2, "老城一号", "老城", 3, 28.5, &["拿铁"], "2026-05-11"),
                item(3, "学校后门", "学校附近", 4, 18.0, &["手冲"], "2026-06-01"),
                item(4, "江边二号", "江边", 4, 40.0, &["可颂"], "2026-09-20"),
                item(5, "雨天书店", "老城", 2, 26.0, &[], "2026-10-02"),
            ],
            months: vec![0, 0, 1, 0, 1, 1, 0, 0, 1, 1, 0, 0],
            n: 5,
            last: 3,
            quotes: vec![MadeQuote { key: "list/3:1".into(), text: "可颂值得为它早起。".into(), from: "备注 · 江边的小窗".into() }],
            ..Default::default()
        }
    }

    #[test]
    fn parts_are_filled_from_the_year() {
        let y = cafes();
        assert_eq!(fill(&y, &Spec::of("big")).unwrap(), json!({ "label": "这一年记了", "n": 5, "unit": "条", "last": 3 }));
        assert_eq!(fill(&y, &Spec { part: "big".into(), label: Some("这一年新去了".into()), unit: Some("家".into()), ..Default::default() }).unwrap()["unit"], "家");
        assert_eq!(fill(&y, &Spec::field("big", "价格")).unwrap()["n"], json!(144.5));
        let stars = fill(&y, &Spec::field("stars", "评分")).unwrap();
        assert_eq!(stars["items"].as_array().unwrap().iter().map(|x| x["title"].as_str().unwrap()).collect::<Vec<_>>(), ["江边的小窗", "学校后门", "江边二号"]);
        assert_eq!(fill(&y, &Spec::field("dist", "区域")).unwrap()["rows"][0], json!({ "name": "江边", "n": 2 }));
        assert_eq!(fill(&y, &Spec::field("chips", "招牌")).unwrap()["rows"], json!([{ "name": "手冲", "n": 2 }, { "name": "可颂", "n": 2 }, { "name": "拿铁", "n": 1 }]));
        let sum = fill(&y, &Spec { part: "sum".into(), field: Some("价格".into()), unit: Some("元".into()), ..Default::default() }).unwrap();
        assert_eq!((sum["rows"].as_array().unwrap().len(), &sum["rows"][3], &sum["total"]), (4, &json!(["还有 2 条", 44]), &json!(144.5)));
        assert_eq!(fill(&y, &Spec { part: "quote".into(), quote: Some("q1".into()), ..Default::default() }).unwrap()["key"], "list/3:1");
        assert_eq!(fill(&y, &Spec::of("cards")).unwrap()["items"][0]["sub"], json!(["江边 · 3 月 2 日"]));
        // what's wrong is told in words 柚子 can act on
        assert_eq!(fill(&y, &Spec::field("stars", "区域")).unwrap_err(), "stars 要评分字段，「区域」是单选");
        assert_eq!(fill(&y, &Spec::field("dist", "城市")).unwrap_err(), "没有叫「城市」的字段");
        assert!(fill(&y, &Spec { part: "quote".into(), quote: Some("q7".into()), ..Default::default() }).is_err());
        assert!(fill(&y, &Spec::of("courses")).is_err());
        assert!(fill(&YearMade { last: 0, ..y }, &Spec::of("compare")).is_err());
    }

    #[test]
    fn layouts_are_checked_and_the_default_always_draws() {
        let y = cafes();
        let slots = |v: &[(&str, Spec)]| v.iter().map(|(s, p)| (s.to_string(), p.clone())).collect::<Vec<_>>();
        let ok = plan_of(&y, "pair", &slots(&[("A", Spec::of("big")), ("B", Spec::field("dist", "区域")), ("S", Spec::of("months"))])).unwrap();
        assert_eq!(ok.parts.iter().map(|p| p.part.as_str()).collect::<Vec<_>>(), ["big", "dist", "months"]);
        let bad = plan_of(&y, "pair", &slots(&[("H", Spec::of("big")), ("A", Spec::of("months")), ("B", Spec::field("stars", "价格"))])).unwrap_err();
        assert_eq!(bad, ["pair 没有 H 这个位置（只有 A、B、S、Q）", "months 不能放在 A（只能放 S）", "B 的 stars：stars 要评分字段，「价格」是数字"]);
        assert!(plan_of(&y, "grid", &[]).is_err());
        // default: count beside the stars, the months, the quote
        let d = default_plan(&y);
        assert_eq!((d.layout.as_str(), d.parts.iter().map(|p| (p.slot.as_str(), p.part.as_str())).collect::<Vec<_>>()), ("pair", vec![("A", "big"), ("B", "stars"), ("S", "months"), ("Q", "quote")]));
        // few items: one slip each
        let few = YearMade { items: y.items[..2].to_vec(), n: 2, quotes: vec![], ..y.clone() };
        assert_eq!(default_plan(&few).layout, "list");
        let course = YearMade {
            template: "progress".into(),
            course_word: "课本".into(),
            lesson_word: "课".into(),
            fields: vec![],
            items: vec![],
            courses: vec![data::MadeCourse { key: "progress/2:1".into(), name: "初级 1".into(), done: 25, total: 25, year: 18, finished: Some("2026-06".into()) }],
            quotes: vec![],
            ..y
        };
        let p = default_plan(&course);
        assert_eq!((p.layout.as_str(), p.parts.len(), &p.parts[0].data["unit"]), ("hero", 2, &json!("课")));
    }

    #[test]
    fn a_reply_is_read_module_by_module() {
        let y = cafes();
        let m = MadeYear { plan: default_plan(&y), mark: "coffee".into(), year: y.clone() };
        let other = MadeYear { year: YearMade { key: "list/5".into(), ..y.clone() }, ..m.clone() };
        let reply = r#"好的：{"modules":[{"key":"list/3","layout":"pair","slots":{"A":"big","B":{"part":"chips","field":"招牌"},"Q":{"part":"quote","ref":"q1"}},"lines":["江边那家去得最多。"]},{"key":"list/5","layout":"pair","slots":{"A":{"part":"months"}}}]}"#;
        let (ok, bad) = read(reply, &[&m, &other]);
        let (_, Some((lines, plan))) = &ok[0] else { panic!() };
        assert_eq!((lines.as_slice(), plan.parts.len()), (&["江边那家去得最多。".to_string()][..], 3));
        assert_eq!(bad, [("list/5".to_string(), "months 不能放在 A（只能放 S）".to_string())]);
        assert_eq!(read(r#"{"modules":[{"key":"list/3","skip":true}]}"#, &[&m]).0, [("list/3".to_string(), None)]);
        assert_eq!(read("不是 JSON", &[&m]).1[0].1, "这一次没有它");
        // the mock lays out what it's given
        let mock = mock_reply(&messages(&[&m, &other], "信")[1].content);
        assert!(read(&mock, &[&m, &other]).1.is_empty(), "{mock}");
    }
}
