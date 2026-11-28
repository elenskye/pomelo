//! PDF 工具箱 (1.2.0, `general-pdf-toolkit`): engine-pdf's own tools for 柚子 — pdf_info, pdf_read, pdf_render, pdf_find,
//! pdf_outline, pdf_mathpix_quote, pdf_mathpix (the rest of the engine's kit is off). Their names, descriptions and
//! results are the engine's; Pomelo only overrides defaults and wires them (engine-pdf `docs/toolkit.md`).
//! - Offered when the question carries a PDF or the conversation has a 知识库 (`skills::Ctx::pdf`); 读文件 and 查知识库
//!   leave PDFs to it. The Mathpix two only with Mathpix's keys (Pub2MD's, in the keychain; mock mode needs none).
//! - Files (`Allowed`): only the question's PDF attachments and project files (their stored copies) and the 知识库's
//!   PDFs (read in place); the model names one by its file name or the path 查知识库 view=files lists (`resolve`).
//! - Page images go to a cloud model, counted with the reply's others (chat.rs `MAX_IMAGES`); a local model gets
//!   pdf_read without images and no pdf_render. pdf_read recognizes scanned pages (`ocr` on by default).
//! - What it reads is numbered as sources with the PDF place (cite.rs `pdf`: physical page, label, pdf_find's boxes).
//!   Long pages the engine put in a text attachment come inline as far as the budget goes.
//! - Mathpix: the engine's spend rule (engine-pdf `mathpix::spend`) decides when to ask; chat.rs asks with a 问我
//!   card and runs the prepared call with the answer (no means nothing is sent). Real uploads go into the engine's
//!   ledger shared by every app, as 「Pomelo」; mock mode uses the engine's simulated Mathpix and a ledger of its own.

use crate::youzi::cite::Sources;
use crate::youzi::data::{clip, Answer};
use crate::youzi::knowledge::KbRef;
use crate::youzi::provider::Image;
use crate::youzi::Mode;
use engine_pdf::mathpix::{self, spend::Ledger};
use engine_pdf::tools::{self, Approve, AttachmentContent, Ctx, Files, Keys, Output, Override, Overrides, Prepared, Prices, Quote, ToolError, ToolSpec, Toolkit};
use engine_pdf::Cancel;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

pub const TOOLS: [&str; 7] = ["pdf_info", "pdf_read", "pdf_render", "pdf_find", "pdf_outline", "pdf_mathpix_quote", "pdf_mathpix"];
pub const MATHPIX: [&str; 2] = ["pdf_mathpix_quote", "pdf_mathpix"];
pub const MATHPIX_RUN: &str = "pdf_mathpix";
/// The account names of the Mathpix keys in the keychain (filled in on Pub2MD's page, used by both).
pub const ID_ACCOUNT: &str = "mathpix-id";
pub const KEY_ACCOUNT: &str = "mathpix-key";
/// A page's text attachment inlined at most (characters).
const INLINE_CHARS: usize = 6000;

pub fn is_tool(name: &str) -> bool {
    TOOLS.contains(&name)
}

/// The engine's kit as 柚子 has it: only `TOOLS`, scanned pages recognized when read.
pub fn toolkit(options: mathpix::Options) -> Toolkit<mathpix::Options> {
    let all = tools::toolkit_with_mathpix(options);
    let mut overrides: Overrides = all.manifest().iter().filter(|s| !is_tool(&s.name)).map(|s| (s.name.clone(), Override { disabled: true, ..Override::default() })).collect();
    let defaults = json!({ "ocr": true });
    overrides.insert("pdf_read".into(), Override { defaults: defaults.as_object().cloned(), ..Override::default() });
    all.with_overrides(&overrides).expect("内置的 PDF 工具覆盖")
}

/// The tool definitions the model gets (without Mathpix when there are no keys).
pub fn definitions(mathpix: bool) -> Vec<Value> {
    let kit = toolkit(mathpix::Options { ledger: Ledger::Off, ..mathpix::Options::default() });
    let specs: Vec<ToolSpec> = kit.manifest().iter().filter(|s| mathpix || !MATHPIX.contains(&s.name.as_str())).cloned().collect();
    tools::to_openai(&specs)
}

/// Mathpix as this run of Pomelo uses it: real uploads into the shared ledger, or (mock) the simulated service.
pub fn options(mode: Mode, data: &Path) -> mathpix::Options {
    match mode {
        Mode::Real => mathpix::Options { app: "Pomelo".into(), ..mathpix::Options::default() },
        Mode::Mock => mathpix::Options {
            app: "Pomelo（开发版）".into(),
            transport: simulated(),
            ledger: Ledger::At(data.join("ledgers")),
            ..mathpix::Options::default()
        },
    }
}

/// The engine's simulated Mathpix, one for the whole run of the app: what was handed to it (Pub2MD's upload) is
/// still there when asked about later (继续 after 停止).
fn simulated() -> std::sync::Arc<dyn mathpix::Transport> {
    static FAKE: std::sync::OnceLock<std::sync::Arc<mathpix::FakeTransport>> = std::sync::OnceLock::new();
    FAKE.get_or_init(|| std::sync::Arc::new(mathpix::FakeTransport::echo())).clone()
}

// ---------- the files it may read ----------

/// A PDF this question may read: its name for the model, its bytes' place, and where a citation points.
#[derive(Debug, Clone, PartialEq)]
pub struct Allowed {
    /// What the model writes as `file`: an attachment's name, a 知识库 file's listed path (「文件夹/相对路径」).
    pub id: String,
    pub path: PathBuf,
    /// An attachment or project file.
    pub artifact: Option<i64>,
    pub kb: Option<KbRef>,
}

impl Allowed {
    fn name(&self) -> &str {
        self.id.rsplit('/').next().unwrap_or(&self.id)
    }
    fn detail(&self) -> String {
        match &self.kb {
            Some(k) => format!("知识库 · {}", k.library),
            None => "附件和资料".into(),
        }
    }
}

/// The file the model named: its id, else its file name, else the one id containing it.
pub fn resolve<'a>(all: &'a [Allowed], want: &str) -> Result<&'a Allowed, String> {
    let want = want.trim().trim_start_matches('/');
    if want.is_empty() {
        return Err("file 写 PDF 的文件名（附件），或查知识库 view=files 列出的路径".into());
    }
    if let Some(a) = all.iter().find(|a| a.id == want) {
        return Ok(a);
    }
    let by_name: Vec<&Allowed> = all.iter().filter(|a| a.name() == want).collect();
    let found = if by_name.is_empty() { all.iter().filter(|a| a.id.contains(want)).collect() } else { by_name };
    match found.as_slice() {
        [one] => Ok(one),
        [] => Err(format!("这次能读的 PDF 里没有「{}」（附件写文件名；知识库里的用查知识库 view=files 找路径）", clip(want, 60))),
        many => Err(format!("有 {} 份 PDF 对得上，file 写完整的路径：{}", many.len(), many.iter().take(8).map(|a| a.id.as_str()).collect::<Vec<_>>().join("、"))),
    }
}

/// What the engine's tools see of Pomelo: the allowed files, the Mathpix keys, and the answer to its question.
pub struct Host<'a> {
    pub allowed: &'a [Allowed],
    pub keys: Option<(Zeroizing<String>, Zeroizing<String>)>,
    /// The user's answer, asked before (chat.rs): what `Approve` says.
    pub yes: bool,
}

impl Files for Host<'_> {
    fn read(&self, path: &str) -> Result<Vec<u8>, ToolError> {
        let a = resolve(self.allowed, path).map_err(|e| ToolError::new("file", e, "换一份这次能读的 PDF。", false))?;
        std::fs::read(&a.path).map_err(|_| ToolError::new("file", "这份 PDF 读不了，可能已经移走或删除了。", "告诉用户文件不在了。", false))
    }
}

impl Keys for Host<'_> {
    fn get(&self, name: &str) -> Option<Zeroizing<String>> {
        match name {
            "mathpix_app_id" => self.keys.as_ref().map(|k| k.0.clone()),
            "mathpix_app_key" => self.keys.as_ref().map(|k| k.1.clone()),
            _ => None, // PDF passwords: not asked for (a locked PDF says so)
        }
    }
}

impl Approve for Host<'_> {
    fn ask(&self, _: &ToolSpec, _: Option<&Quote>) -> bool {
        self.yes
    }
}

impl Host<'_> {
    pub fn ctx<'a>(&'a self, cancel: &'a Cancel, prices: &'a Prices) -> Ctx<'a> {
        Ctx { files: self, keys: self, approve: self, cancel, prices, progress: &|_| {} }
    }
}

/// The model's arguments as this target may run them: a local model reads without images and can't render a page; a
/// cloud model's images are capped at `room`. Err: what to tell the model instead.
pub fn adjust(name: &str, mut args: Value, cloud: bool, room: usize) -> Result<Value, String> {
    match name {
        "pdf_render" if !cloud => Err("现在用的是本地模型，看不了页图：用 pdf_read 读这一页的文字（告诉用户要看图得换云端模型）。".into()),
        "pdf_render" if room == 0 => Err("这条回答已经看了够多的图片，不能再看了：用 pdf_read 读文字。".into()),
        "pdf_read" => {
            if !cloud || room == 0 {
                args["images"] = json!(false);
            } else {
                let asked = args["max_images"].as_u64().unwrap_or(room as u64);
                args["max_images"] = json!(asked.min(room as u64));
            }
            Ok(args)
        }
        _ => Ok(args),
    }
}

/// Runs one call (not pdf_mathpix after a question: `prepare` then `run_prepared`). Slow: off the main thread.
pub fn call(kit: &Toolkit<mathpix::Options>, name: &str, args: &Value, host: &Host) -> Result<Output, ToolError> {
    let (cancel, prices) = (Cancel::new(), Prices::default());
    kit.call(name, args, &host.ctx(&cancel, &prices))
}

pub fn prepare(kit: &Toolkit<mathpix::Options>, name: &str, args: &Value, host: &Host) -> Result<Prepared, ToolError> {
    let (cancel, prices) = (Cancel::new(), Prices::default());
    kit.prepare(name, args, &host.ctx(&cancel, &prices))
}

pub fn run_prepared(kit: &Toolkit<mathpix::Options>, prepared: Prepared, host: &Host) -> Result<Output, ToolError> {
    let (cancel, prices) = (Cancel::new(), Prices::default());
    kit.run(if host.yes { prepared.approve() } else { prepared }, &host.ctx(&cancel, &prices))
}

/// What a failed call tells the model (and the 「查了」 entry).
pub fn failed(name: &str, e: &ToolError) -> Answer {
    let spent = e.usage.as_ref().filter(|u| u.usd > 0.0).map(|u| format!("（已花 ${:.3}）", u.usd)).unwrap_or_default();
    Answer { module: None, label: format!("PDF · {name} · 没成{spent}"), state: "failed", text: format!("没成：{}{spent}\n下一步：{}", e.message, e.hint), records: vec![] }
}

fn base64(bytes: &[u8]) -> String {
    crate::youzi::office::base64(bytes)
}

fn rects_of(v: &Value) -> Option<Vec<[f64; 4]>> {
    let list: Vec<[f64; 4]> = v.as_array()?.iter().filter_map(|r| Some([r["x"].as_f64()?, r["y"].as_f64()?, r["width"].as_f64()?, r["height"].as_f64()?])).collect();
    (!list.is_empty()).then_some(list)
}

/// A call's result as the model gets it: the engine's text, its long pages inline, what it read numbered as sources
/// (with the PDF place), and its page images (at most `room`, for a cloud model; `before`: images already queued for
/// the next message, so the numbers say which ones are these).
pub fn answer(name: &str, file: Option<&Allowed>, out: Output, sources: &mut Sources, cloud: bool, room: usize, before: usize, budget: i64) -> (Answer, Vec<Image>) {
    let mut text = out.text.trim_end().to_string();
    let data = &out.data;
    let cite = |sources: &mut Sources, page: i64, label: Option<&str>, rects: Option<Vec<[f64; 4]>>, content: &str| -> Option<i64> {
        let f = file?;
        let title = format!("{} · 第 {page} 页", f.name());
        Some(sources.pdf(f.artifact, f.kb.as_ref(), &title, &f.detail(), page, label, rects, content))
    };
    let mut numbers: Vec<String> = vec![];
    let mut label = format!("PDF · {name}");
    match name {
        "pdf_read" => {
            let pages = data["pages"].as_array().cloned().unwrap_or_default();
            for p in &pages {
                let page = p["index"].as_i64().unwrap_or(0);
                let content = p["text"].as_str().unwrap_or("");
                if let Some(n) = cite(sources, page, p["label"].as_str(), None, &clip(content, 600)) {
                    numbers.push(format!("第 {page} 页 [{n}]"));
                }
            }
            let range = match (pages.first().and_then(|p| p["index"].as_i64()), pages.last().and_then(|p| p["index"].as_i64())) {
                (Some(a), Some(b)) if a != b => format!("第 {a}–{b} 页"),
                (Some(a), _) => format!("第 {a} 页"),
                _ => String::new(),
            };
            label = format!("PDF · 读《{}》{range}", file.map(Allowed::name).unwrap_or(""));
        }
        "pdf_find" => {
            let hits = data["hits"].as_array().cloned().unwrap_or_default();
            for (i, h) in hits.iter().enumerate() {
                let page = h["location"]["page"].as_i64().unwrap_or(0);
                let content = format!("{}{}{}", h["before"].as_str().unwrap_or(""), h["matched"].as_str().unwrap_or(""), h["after"].as_str().unwrap_or(""));
                if let Some(n) = cite(sources, page, h["location"]["label"].as_str(), rects_of(&h["location"]["rects"]), &content) {
                    numbers.push(format!("第 {} 处（第 {page} 页）[{n}]", i + 1));
                }
            }
            label = format!("PDF · 在《{}》里找 · {} 处", file.map(Allowed::name).unwrap_or(""), data["total"].as_u64().unwrap_or(hits.len() as u64));
        }
        "pdf_render" => {
            let page = data["page"].as_i64().unwrap_or(0);
            if let Some(n) = cite(sources, page, None, None, "（看的是这一页的页图）") {
                numbers.push(format!("第 {page} 页 [{n}]"));
            }
            label = format!("PDF · 看《{}》第 {page} 页", file.map(Allowed::name).unwrap_or(""));
        }
        "pdf_mathpix" => {
            // the engine's text has a part per page (「## 第 3 页」 and how it starts)
            for part in text.split("\n## 第 ").skip(1) {
                let (head, body) = part.split_once('\n').unwrap_or((part, ""));
                if let Some(page) = head.split(' ').next().and_then(|n| n.parse::<i64>().ok()) {
                    if let Some(n) = cite(sources, page, None, None, &clip(body.trim(), 600)) {
                        numbers.push(format!("第 {page} 页 [{n}]"));
                    }
                }
            }
            let usd = out.usage.as_ref().map_or(0.0, |u| u.usd);
            label = format!("PDF · Mathpix 识别《{}》{} 页 · ${usd:.3}", file.map(Allowed::name).unwrap_or(""), data["pages"].as_array().map_or(0, Vec::len));
        }
        "pdf_mathpix_quote" => label = format!("PDF · Mathpix 报价 · ${:.3}", data["usd"].as_f64().unwrap_or(0.0)),
        "pdf_outline" => label = format!("PDF · 《{}》的目录", file.map(Allowed::name).unwrap_or("")),
        "pdf_info" => label = format!("PDF · 《{}》的信息", file.map(Allowed::name).unwrap_or("")),
        _ => {}
    }
    // attachments: page images to a cloud model; long text inline
    let mut images = vec![];
    let mut used = crate::youzi::context::estimate(&text);
    for a in &out.attachments {
        let AttachmentContent::Bytes(bytes) = &a.content else { continue };
        if a.mime_type.starts_with("image/") {
            if cloud && images.len() < room {
                images.push(Image { media_type: a.mime_type.clone(), data: base64(bytes) });
                text.push_str(&format!("\n`{}` 是下面那条消息里的第 {} 张图。", a.filename, before + images.len()));
            }
        } else if a.mime_type.starts_with("text/") {
            let full = String::from_utf8_lossy(bytes);
            let piece = format!("\n\n`{}`：\n{}", a.filename, clip(&full, INLINE_CHARS));
            let cost = crate::youzi::context::estimate(&piece);
            if used + cost <= budget {
                used += cost;
                text.push_str(&piece);
            } else {
                text.push_str(&format!("\n\n`{}` 太长放不下：用 pdf_find 找需要的地方。", a.filename));
            }
        }
    }
    if let Some(next) = &out.next {
        text.push_str(&format!("\n\n后面还有：接着读把这些参数原样再调用一次 {name}：{next}"));
    }
    if !numbers.is_empty() {
        text.push_str(&format!("\n\n引用时标：{}", numbers.join("、")));
    }
    if crate::youzi::context::estimate(&text) > budget {
        text = clip(&text, (budget.max(200) * 2) as usize);
    }
    (Answer { module: None, label, state: "ok", text, records: vec![] }, images)
}

/// The 问我 card before a paid call the spend rule asks about.
pub fn ask_question(file: &str, quote: &Quote, why: &str) -> crate::youzi::ask::Question {
    crate::youzi::ask::Question {
        question: format!("送 Mathpix 识别《{file}》的 {} 页？预计 ${:.3}。{why}", quote.units, quote.usd),
        kind: "judge".into(),
        options: vec!["是".into(), "否".into()],
    }
}

/// The card was answered 是.
pub fn said_yes(answer: &str) -> bool {
    answer.lines().any(|l| l.trim() == "→ 是")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allowed(dir: &Path) -> Vec<Allowed> {
        let pdf = crate::youzi::pdf::tests::sample_pdf(&["Annual leave policy details", "Unused days carry over until March"]);
        std::fs::write(dir.join("a.pdf"), &pdf).unwrap();
        std::fs::write(dir.join("b.pdf"), &pdf).unwrap();
        vec![
            Allowed { id: "合同.pdf".into(), path: dir.join("a.pdf"), artifact: Some(7), kb: None },
            Allowed { id: "资料/2026/合同.pdf".into(), path: dir.join("b.pdf"), artifact: None, kb: Some(KbRef { source: 3, path: "2026/合同.pdf".into(), library: "工作".into() }) },
            Allowed { id: "资料/假期.pdf".into(), path: dir.join("a.pdf"), artifact: None, kb: Some(KbRef { source: 3, path: "假期.pdf".into(), library: "工作".into() }) },
        ]
    }

    #[test]
    fn only_the_questions_and_the_knowledge_bases_pdfs_are_read() {
        let dir = crate::db::tests::temp_dir("pdfkit-files");
        let all = allowed(&dir);
        assert_eq!(resolve(&all, "资料/2026/合同.pdf").unwrap().kb.as_ref().unwrap().source, 3);
        assert_eq!(resolve(&all, "假期.pdf").unwrap().id, "资料/假期.pdf");
        assert!(resolve(&all, "合同.pdf").is_ok()); // the attachment's own name wins as an id
        assert!(resolve(&all, "2026").is_ok());
        assert!(resolve(&all, "/etc/passwd").unwrap_err().contains("没有"));
        assert!(resolve(&all, "资料").unwrap_err().contains("对得上"));
        let host = Host { allowed: &all, keys: None, yes: false };
        assert_eq!(host.read("/Users/someone/secret.pdf").unwrap_err().kind, "file");
        assert!(host.read("合同.pdf").unwrap().starts_with(b"%PDF"));
    }

    #[test]
    fn a_local_model_reads_without_images_and_a_cloud_one_within_the_room_left() {
        assert!(adjust("pdf_render", json!({}), false, 8).is_err());
        assert!(adjust("pdf_render", json!({}), true, 0).is_err());
        assert_eq!(adjust("pdf_read", json!({ "file": "a" }), false, 8).unwrap()["images"], false);
        assert_eq!(adjust("pdf_read", json!({ "max_images": 9 }), true, 3).unwrap()["max_images"], 3);
        assert_eq!(adjust("pdf_find", json!({ "query": "x" }), false, 0).unwrap()["query"], "x");
    }

    #[test]
    fn reads_and_finds_become_page_citations_with_boxes_and_images() {
        let dir = crate::db::tests::temp_dir("pdfkit-run");
        let all = allowed(&dir);
        let kit = toolkit(mathpix::Options { ledger: Ledger::Off, ..mathpix::Options::default() });
        let names: Vec<&str> = kit.manifest().iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names.len(), TOOLS.len());
        assert!(definitions(false).iter().all(|d| !MATHPIX.contains(&d["function"]["name"].as_str().unwrap())));
        assert_eq!(definitions(true).len(), 7);
        let host = Host { allowed: &all, keys: None, yes: false };
        let mut sources = Sources::default();
        // read two pages with images (a cloud model with room for one)
        let args = adjust("pdf_read", json!({ "file": "假期.pdf", "pages": [1, 2] }), true, 1).unwrap();
        let out = call(&kit, "pdf_read", &args, &host).unwrap();
        let (a, images) = answer("pdf_read", resolve(&all, "假期.pdf").ok(), out, &mut sources, true, 1, 2, 4000);
        assert_eq!(a.label, "PDF · 读《假期.pdf》第 1–2 页");
        assert!(a.text.contains("第 1 页 [1]、第 2 页 [2]") && a.text.contains("第 3 张图"), "{}", a.text);
        assert_eq!(images.len(), 1);
        assert_eq!((sources.0[0].pdf_page, sources.0[0].kb_source, sources.0[0].title.as_str()), (Some(1), Some(3), "假期.pdf · 第 1 页"));
        // find: the hit's boxes go with its citation
        let out = call(&kit, "pdf_find", &json!({ "file": "合同.pdf", "query": "Unused days" }), &host).unwrap();
        let (a, _) = answer("pdf_find", resolve(&all, "合同.pdf").ok(), out, &mut sources, false, 0, 0, 4000);
        assert!(a.text.contains("第 1 处（第 2 页）[3]"), "{}", a.text);
        let hit = &sources.0[2];
        assert_eq!((hit.artifact_id, hit.pdf_page), (Some(7), Some(2)));
        assert!(hit.pdf_rects.as_ref().is_some_and(|r| !r.is_empty() && r[0][2] > 0.0));
        // a local model: no image is sent
        let out = call(&kit, "pdf_read", &adjust("pdf_read", json!({ "file": "合同.pdf", "pages": [1] }), false, 8).unwrap(), &host).unwrap();
        let (_, images) = answer("pdf_read", resolve(&all, "合同.pdf").ok(), out, &mut sources, false, 8, 0, 4000);
        assert!(images.is_empty());
    }

    #[test]
    fn mathpix_asks_by_the_engines_rule_and_no_sends_nothing() {
        let dir = crate::db::tests::temp_dir("pdfkit-mathpix");
        let all = allowed(&dir);
        let mut options = options(Mode::Mock, &dir);
        options.ledger = Ledger::At(dir.to_path_buf());
        let book = options.book();
        let kit = toolkit(options);
        let keys = Some((Zeroizing::new("id".to_string()), Zeroizing::new("key".to_string())));
        let host = Host { allowed: &all, keys: keys.clone(), yes: false };
        let args = json!({ "file": "合同.pdf", "pages": [2] });
        // $0.005: no question, it runs (simulated: nothing recorded)
        let prepared = prepare(&kit, MATHPIX_RUN, &args, &host).unwrap();
        assert!(!prepared.needs_confirm() && prepared.why().is_none());
        let out = run_prepared(&kit, prepared, &host).unwrap();
        assert!(book.entries(std::time::UNIX_EPOCH).is_empty());
        let mut sources = Sources::default();
        let (a, _) = answer(MATHPIX_RUN, resolve(&all, "合同.pdf").ok(), out, &mut sources, true, 8, 0, 4000);
        assert!(a.label.starts_with("PDF · Mathpix 识别《合同.pdf》1 页"), "{}", a.label);
        // the last day near $5 (every app): asked; 否 → denied, nothing sent
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        book.record(&mathpix::spend::Entry { at: now, app: "Peck".into(), service: "Mathpix".into(), unit: "page".into(), units: 1000.0, usd: 4.999, estimated: false }).unwrap();
        let prepared = prepare(&kit, MATHPIX_RUN, &args, &host).unwrap();
        assert!(prepared.needs_confirm());
        let why = prepared.why().copied().unwrap();
        let q = ask_question("合同.pdf", prepared.quote().unwrap(), &why.to_string());
        assert!(q.question.contains("最近 24 小时已花"), "{}", q.question);
        assert_eq!(run_prepared(&kit, prepared.clone(), &host).unwrap_err().kind, "denied");
        // 是 → approved and run
        let yes = Host { allowed: &all, keys, yes: true };
        assert!(run_prepared(&kit, prepared, &yes).is_ok());
        assert!(said_yes("我的回答：\n送 Mathpix …？\n→ 是") && !said_yes("我的回答：\n送 Mathpix …？\n→ 否"));
    }
}

/// Real files: `POMELO_PDF_FILE` searched for `POMELO_PDF_QUERY` with pdf_find, the first hit's page drawn with its
/// boxes → `POMELO_PDF_DUMP` (an HTML page to look at).
#[cfg(test)]
mod real {
    use super::*;

    #[test]
    #[ignore]
    fn find_and_draw() {
        let (Ok(file), Ok(query), Ok(out)) = (std::env::var("POMELO_PDF_FILE"), std::env::var("POMELO_PDF_QUERY"), std::env::var("POMELO_PDF_DUMP")) else { return };
        let all = vec![Allowed { id: "x.pdf".into(), path: PathBuf::from(&file), artifact: Some(1), kb: None }];
        let kit = toolkit(mathpix::Options { ledger: Ledger::Off, ..mathpix::Options::default() });
        let out_find = call(&kit, "pdf_find", &json!({ "file": "x.pdf", "query": query }), &Host { allowed: &all, keys: None, yes: false }).unwrap();
        let mut sources = Sources::default();
        let (a, _) = answer("pdf_find", all.first(), out_find, &mut sources, false, 0, 0, 8000);
        println!("{}\n{}", a.label, a.text);
        let s = &sources.0[0];
        let page = crate::youzi::cite::pdf_page(&all[0].path, s.pdf_page.unwrap() as u32, s.pdf_rects.as_deref().unwrap_or(&[])).unwrap();
        let boxes: String = page.boxes.iter().map(|b| format!("<span style=\"position:absolute;left:{}%;top:{}%;width:{}%;height:{}%;background:rgba(255,220,0,.45);outline:1px solid red\"></span>", b[0] * 100.0, b[1] * 100.0, b[2] * 100.0, b[3] * 100.0)).collect();
        std::fs::write(out, format!("<!doctype html><meta charset=utf-8><p>{} · 第 {} 页 · {:?}</p><div style=\"position:relative;width:900px\"><img src=\"{}\" style=\"width:100%;display:block\">{boxes}</div>", s.title, s.pdf_page.unwrap(), page.label, page.image)).unwrap();
    }
}
