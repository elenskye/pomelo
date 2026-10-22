//! Mathpix (Pub2MD 提取): a PDF or Word file (images are packed into a PDF first, pdf.rs) becomes Mathpix
//! Markdown with `\pagebreak` between pages, plus its .mmd.zip (the text and the cropped images).
//! - Only https://api.mathpix.com; the App ID and App Key live in the keychain (KeyStore accounts
//!   below), never in a job file, the log or an error.
//! - submit: one multipart POST /v3/pdf (the body built here). Never asked again by itself (each upload
//!   is charged): the job keeps the pdf_id at once, so 继续 only waits and downloads.
//! - status: GET /v3/pdf/{id} (`status`, pages done / pages); download: GET /v3/pdf/{id}.mmd and
//!   .mmd.zip (202 = not assembled yet: ask again after a while).
//! - convert: POST /v3/converter {mmd, formats} (images by their links), GET /v3/converter/{id} for each
//!   format's status, GET /v3/converter/{id}.{ext}; the files aren't kept there long: fetched at once.
//! - Service errors are said in fixed Chinese (never the service's own text).
//! Mock (dev / tests): the PDF's own text per page (PDFKit), a Word file's text, a line per image.

use serde_json::{json, Value};
use std::time::Duration;

pub const ID_ACCOUNT: &str = "mathpix-id";
pub const KEY_ACCOUNT: &str = "mathpix-key";
const API: &str = "https://api.mathpix.com";
/// Mathpix's own limit.
pub const MAX_UPLOAD: usize = 1_000_000_000;

#[derive(Clone)]
pub struct Keys {
    pub id: String,
    pub key: String,
}

/// What Mathpix is asked for: page breaks (the chunks know their pages), headers and footers kept
/// (整理 decides; nothing lost), no use of the file to improve Mathpix.
pub fn options() -> Value {
    json!({ "include_page_breaks": true, "include_page_info": true, "metadata": { "improve_mathpix": false } })
}

/// A multipart/form-data body: the file (under a fixed name, never the local path) and options_json.
pub fn multipart(boundary: &str, file_name: &str, mime: &str, data: &[u8], options: &Value) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 512);
    out.extend(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"options_json\"\r\n\r\n{options}\r\n").as_bytes());
    out.extend(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{file_name}\"\r\nContent-Type: {mime}\r\n\r\n").as_bytes());
    out.extend(data);
    out.extend(format!("\r\n--{boundary}--\r\n").as_bytes());
    out
}

fn client(timeout: u64) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(timeout))
        .build()
        .map_err(|_| NETWORK.to_string())
}

const NETWORK: &str = "连不上 Mathpix，请检查网络。";

fn http_failure(status: u16) -> String {
    match status {
        401 | 403 => "Mathpix 的 App ID 或 App Key 不对。".into(),
        402 => "Mathpix 账户的额度不够了。".into(),
        429 => "Mathpix 请求太频繁，过一会儿再试。".into(),
        400 | 413 | 415 | 422 => "Mathpix 不接受这个文件（可能是加了密、损坏，或者格式不支持）。".into(),
        500..=599 => "Mathpix 暂时不可用，过一会儿再试。".into(),
        s => format!("Mathpix 返回了错误（HTTP {s}）。"),
    }
}

/// The error a response body carries (`error` / `error_info`), said in Chinese.
fn body_error(v: &Value) -> Option<String> {
    let id = v["error_info"]["id"].as_str().unwrap_or("");
    if v.get("error").is_none_or(Value::is_null) && v.get("error_info").is_none_or(Value::is_null) {
        return None;
    }
    Some(match id {
        "pdf_encrypted" => "这个 PDF 加了密，Mathpix 读不了。先去掉密码再放进来。".into(),
        "pdf_page_limit_exceeded" | "pdf_too_many_pages" => "页数超过了 Mathpix 的上限。".into(),
        "image_download_error" | "pdf_download_error" | "http_download_error" => "Mathpix 读不到这个文件。".into(),
        "opts_unknown_ocr" | "opts_bad_callback" => "Mathpix 不接受这次的设置。".into(),
        "sys_exception" | "sys_request_too_large" => "Mathpix 处理时出错了，过一会儿再试。".into(),
        _ => "Mathpix 没能识别这个文件。".into(),
    })
}

/// Uploads the file; returns the pdf_id (keep it at once: asking again is charged again).
pub async fn submit(keys: &Keys, file_name: &str, mime: &str, data: &[u8]) -> Result<String, String> {
    if data.len() > MAX_UPLOAD {
        return Err("文件超过了 Mathpix 的 1 GB 上限。".into());
    }
    let boundary = format!("pomelo-{:016x}", random());
    let body = multipart(&boundary, file_name, mime, data, &options());
    let resp = client(900)?
        .post(format!("{API}/v3/pdf"))
        .header("app_id", &keys.id)
        .header("app_key", &keys.key)
        .header("Content-Type", format!("multipart/form-data; boundary={boundary}"))
        .body(body)
        .send()
        .await
        .map_err(|_| NETWORK.to_string())?;
    let status = resp.status().as_u16();
    let v: Value = resp.json().await.unwrap_or(Value::Null);
    if !(200..300).contains(&status) {
        return Err(body_error(&v).unwrap_or_else(|| http_failure(status)));
    }
    if let Some(e) = body_error(&v) {
        return Err(e);
    }
    v["pdf_id"].as_str().filter(|id| valid_id(id)).map(str::to_string).ok_or_else(|| "Mathpix 返回了看不懂的结果。".to_string())
}

pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 200 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

#[derive(Debug, Clone, PartialEq)]
pub struct Status {
    /// received | loaded | split | completed | error
    pub state: String,
    pub pages: Option<u64>,
    pub done: Option<u64>,
}

pub async fn status(keys: &Keys, id: &str) -> Result<Status, String> {
    let resp = client(60)?.get(format!("{API}/v3/pdf/{id}")).header("app_id", &keys.id).header("app_key", &keys.key).send().await.map_err(|_| NETWORK.to_string())?;
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(http_failure(status));
    }
    let v: Value = resp.json().await.map_err(|_| "Mathpix 返回了看不懂的结果。".to_string())?;
    let state = v["status"].as_str().unwrap_or("").to_string();
    if state == "error" {
        return Err(body_error(&v).unwrap_or_else(|| "Mathpix 没能识别这个文件。".into()));
    }
    Ok(Status { state, pages: v["num_pages"].as_u64(), done: v["num_pages_completed"].as_u64() })
}

/// One output (`mmd`, `mmd.zip`); None = not ready yet.
pub async fn download(keys: &Keys, id: &str, ext: &str) -> Result<Option<Vec<u8>>, String> {
    fetch(keys, &format!("pdf/{id}.{ext}")).await
}

/// A file under /v3/; None = not ready yet (202, 404, or a JSON answer instead of the file).
async fn fetch(keys: &Keys, path: &str) -> Result<Option<Vec<u8>>, String> {
    let resp = client(600)?.get(format!("{API}/v3/{path}")).header("app_id", &keys.id).header("app_key", &keys.key).send().await.map_err(|_| NETWORK.to_string())?;
    match resp.status().as_u16() {
        202 | 404 => Ok(None),
        200 => {
            let mime = resp.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("").to_lowercase();
            if mime.contains("json") {
                return Ok(None);
            }
            Ok(Some(resp.bytes().await.map_err(|_| "从 Mathpix 下载结果时断了。".to_string())?.to_vec()))
        }
        s => Err(http_failure(s)),
    }
}

/// The converter: a document (MMD) to these formats (Mathpix keys: docx, pdf, tex.zip, html.zip). Returns
/// the conversion id.
pub async fn convert(keys: &Keys, mmd: &str, formats: &[&str]) -> Result<String, String> {
    let body = json!({ "mmd": mmd, "formats": formats.iter().map(|f| (f.to_string(), json!(true))).collect::<serde_json::Map<_, _>>(), "metadata": { "improve_mathpix": false } });
    if body.to_string().len() > 10_000_000 {
        return Err("文件超过了 Mathpix 转格式的 10 MB 上限".into());
    }
    let resp = client(120)?.post(format!("{API}/v3/converter")).header("app_id", &keys.id).header("app_key", &keys.key).json(&body).send().await.map_err(|_| NETWORK.to_string())?;
    let status = resp.status().as_u16();
    let v: Value = resp.json().await.unwrap_or(Value::Null);
    if !(200..300).contains(&status) {
        return Err(body_error(&v).unwrap_or_else(|| http_failure(status)));
    }
    if let Some(e) = body_error(&v) {
        return Err(e);
    }
    v["conversion_id"].as_str().filter(|id| valid_id(id)).map(str::to_string).ok_or_else(|| "Mathpix 返回了看不懂的结果。".to_string())
}

/// Each format's state: processing | completed | error.
pub async fn conversion(keys: &Keys, id: &str) -> Result<Vec<(String, String)>, String> {
    let resp = client(60)?.get(format!("{API}/v3/converter/{id}")).header("app_id", &keys.id).header("app_key", &keys.key).send().await.map_err(|_| NETWORK.to_string())?;
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(http_failure(status));
    }
    let v: Value = resp.json().await.map_err(|_| "Mathpix 返回了看不懂的结果。".to_string())?;
    Ok(conversion_states(&v))
}

fn conversion_states(v: &Value) -> Vec<(String, String)> {
    v["conversion_status"].as_object().map(|m| m.iter().map(|(k, s)| (k.clone(), s["status"].as_str().unwrap_or("processing").to_string())).collect()).unwrap_or_default()
}

/// A converted file; None = not ready yet.
pub async fn converted(keys: &Keys, id: &str, ext: &str) -> Result<Option<Vec<u8>>, String> {
    fetch(keys, &format!("converter/{id}.{ext}")).await.map(|b| b.filter(|b| !b.is_empty()))
}

fn random() -> u64 {
    let mut b = [0u8; 8];
    let _ = getrandom::fill(&mut b);
    u64::from_le_bytes(b)
}

// ---------- mock ----------

/// What the mock "recognizes": a PDF's text page by page, a Word file's text, a line per image.
pub fn mock_mmd(kind: super::source::Kind, paths: &[std::path::PathBuf], pdf: Option<&[u8]>) -> Result<String, String> {
    use super::source::Kind;
    let pages: Vec<String> = match kind {
        Kind::Pdf => crate::picky::pdf::pages(pdf.ok_or("没有 PDF")?)?.into_iter().map(|t| mock_page(&t)).collect(),
        Kind::Word => vec![crate::picky::office::word_text(&paths[0])?],
        Kind::Images => paths.iter().map(|p| format!("（模拟识别的图片：{}）", p.file_name().and_then(|n| n.to_str()).unwrap_or(""))).collect(),
        Kind::Text => vec![std::fs::read_to_string(&paths[0]).map_err(|e| e.to_string())?],
    };
    Ok(pages.join("\n\n\\pagebreak\n\n"))
}

/// A page's text as paragraphs (PDFKit gives lines).
fn mock_page(text: &str) -> String {
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    if lines.is_empty() {
        return "（这一页没有文字）".into();
    }
    lines.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multipart_body_and_options() {
        let body = multipart("b0", "input.pdf", "application/pdf", b"%PDF-1.4 x", &options());
        let text = String::from_utf8_lossy(&body);
        assert!(text.starts_with("--b0\r\nContent-Disposition: form-data; name=\"options_json\"\r\n\r\n{"));
        assert!(text.contains("\"include_page_breaks\":true") && text.contains("\"improve_mathpix\":false"));
        assert!(text.contains("name=\"file\"; filename=\"input.pdf\"\r\nContent-Type: application/pdf\r\n\r\n%PDF-1.4 x\r\n--b0--\r\n"));
        assert!(valid_id("2026_10_21-abc") && !valid_id("../x") && !valid_id(""));
    }

    #[test]
    fn conversion_states_per_format() {
        let v = json!({ "status": "completed", "conversion_status": { "docx": { "status": "completed" }, "pdf": { "status": "processing" } } });
        let mut s = conversion_states(&v);
        s.sort();
        assert_eq!(s, [("docx".to_string(), "completed".to_string()), ("pdf".to_string(), "processing".to_string())]);
        assert!(conversion_states(&json!({})).is_empty());
    }

    #[test]
    fn errors_are_said_in_chinese() {
        assert!(http_failure(401).contains("App Key"));
        assert!(body_error(&json!({ "error": "x", "error_info": { "id": "pdf_encrypted", "message": "secret detail" } })).unwrap().contains("加了密"));
        assert!(!body_error(&json!({ "error": "boom" })).unwrap().contains("boom"));
        assert!(body_error(&json!({ "pdf_id": "x" })).is_none());
        assert!(body_error(&json!({ "pdf_id": "x", "error": null })).is_none());
    }
}
