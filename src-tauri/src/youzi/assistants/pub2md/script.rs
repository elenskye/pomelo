//! 简繁互转 (Pub2MD): OpenCC's dictionaries first (ferrous-opencc, built in), with the regions' words
//! (Taiwan's 軟體、資訊 ↔ the mainland's 软件、信息) and quotation marks (「」『』 ↔ “”‘’); the model
//! looks the draft over after (job.rs) and changes the words the dictionaries don't know (川普 ↔ 特朗普). Code (fenced blocks, `inline`), link and image targets (`](…)`,
//! `\includegraphics{…}`) and web addresses are left as they are: a file name in Chinese must still match.
//! `simplified`: an English → 简体 translation's stray traditional characters, by character (OpenCC t2s; the
//! regions' words would also turn 建立 into 创建).

use ferrous_opencc::config::BuiltinConfig;
use ferrous_opencc::OpenCC;
use std::sync::OnceLock;

fn converter(to_traditional: bool) -> Option<&'static OpenCC> {
    static S2T: OnceLock<Option<OpenCC>> = OnceLock::new();
    static T2S: OnceLock<Option<OpenCC>> = OnceLock::new();
    let cell = if to_traditional { &S2T } else { &T2S };
    cell.get_or_init(|| OpenCC::from_config(if to_traditional { BuiltinConfig::S2twp } else { BuiltinConfig::Tw2sp }).ok()).as_ref()
}

/// The text in the other script (code untouched).
pub fn convert(text: &str, to_traditional: bool) -> Result<String, String> {
    let cc = converter(to_traditional).ok_or("简繁转换的词典没能加载")?;
    Ok(outside_code(text, |t| quotes(&cc.convert(t), to_traditional)))
}

/// Simplified text's traditional characters made simplified, nothing else (code untouched).
pub fn simplified(text: &str) -> Result<String, String> {
    static CHARS: OnceLock<Option<OpenCC>> = OnceLock::new();
    let cc = CHARS.get_or_init(|| OpenCC::from_config(BuiltinConfig::T2s).ok()).as_ref().ok_or("简繁转换的词典没能加载")?;
    Ok(outside_code(text, |t| cc.convert(t)))
}

/// `f` on the text, not on code (fenced blocks, `inline`) or link / image targets.
fn outside_code(text: &str, f: impl Fn(&str) -> String) -> String {
    let mut out = String::with_capacity(text.len());
    let mut fenced = false;
    for line in text.split_inclusive('\n') {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            out.push_str(line);
            continue;
        }
        if fenced {
            out.push_str(line);
            continue;
        }
        // `inline code` stays
        for (i, part) in line.split('`').enumerate() {
            if i > 0 {
                out.push('`');
            }
            if i % 2 == 1 { out.push_str(part) } else { out.push_str(&outside_targets(part, &f)) }
        }
    }
    out
}

/// The region's quotation marks: 「」『』 in Taiwan, “”‘’ on the mainland (an English apostrophe stays).
fn quotes(text: &str, to_traditional: bool) -> String {
    if !to_traditional {
        return text.replace('「', "“").replace('」', "”").replace('『', "‘").replace('』', "’");
    }
    let c: Vec<char> = text.chars().collect();
    let apostrophe = |i: usize| i > 0 && c[i - 1].is_ascii_alphanumeric() && c.get(i + 1).is_some_and(char::is_ascii_alphabetic);
    c.iter()
        .enumerate()
        .map(|(i, &ch)| match ch {
            '“' => '「',
            '”' => '」',
            '‘' if !apostrophe(i) => '『',
            '’' if !apostrophe(i) => '』',
            _ => ch,
        })
        .collect()
}

/// `f` on the text, not on link / image targets and web addresses.
fn outside_targets(text: &str, f: impl Fn(&str) -> String) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    loop {
        let starts = [rest.find("](").map(|i| (i + 2, ')')), rest.find("\\includegraphics").and_then(|i| rest[i..].find('{').map(|j| (i + j + 1, '}'))), rest.find("http://").or_else(|| rest.find("https://")).map(|i| (i, ' '))];
        let Some((a, close)) = starts.into_iter().flatten().min_by_key(|(a, _)| *a) else { break };
        let end = if close == ' ' { rest[a..].find(|c: char| c.is_whitespace() || ")]>\"'".contains(c)) } else { rest[a..].find(close) }.map_or(rest.len(), |e| a + e);
        out.push_str(&f(&rest[..a]));
        out.push_str(&rest[a..end]);
        rest = &rest[end..];
    }
    out.push_str(&f(rest));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_ways_and_code_stays() {
        assert_eq!(convert("开放中文转换是完全由 Rust 实现的。", true).unwrap(), "開放中文轉換是完全由 Rust 實現的。");
        assert_eq!(convert("開放中文轉換", false).unwrap(), "开放中文转换");
        assert_eq!(convert("变量 `转换` 和\n```\n转换\n```\n转换", true).unwrap(), "變數 `转换` 和\n```\n转换\n```\n轉換");
        assert_eq!(convert("公式 \\(x_{转}\\) ok", true).unwrap(), "公式 \\(x_{轉}\\) ok");
        // file names and addresses stay, the words around them don't
        assert_eq!(
            convert("图：![转换流程](图/转换.png) 和 \\includegraphics[width=1]{图/转换.jpg} 见 https://例子.com/转换 转换", true).unwrap(),
            "圖：![轉換流程](图/转换.png) 和 \\includegraphics[width=1]{图/转换.jpg} 見 https://例子.com/转换 轉換"
        );
        // a stray traditional character in an English → 简体 translation (job.rs)
        assert_eq!(simplified("为中東建立新的安全机制，「引号」和`中東` 照留").unwrap(), "为中东建立新的安全机制，「引号」和`中東` 照留");
        // the readers' words and quotation marks
        assert_eq!(convert("「把製造業帶回美國」的軟體和資訊，『引號』", false).unwrap(), "“把制造业带回美国”的软件和信息，‘引号’");
        assert_eq!(convert("“把制造业带回美国”的软件和信息，‘引号’，Nvidia’s chips", true).unwrap(), "「把製造業帶回美國」的軟體和資訊，『引號』，Nvidia’s chips");
    }

    /// Traditional characters in earlier English → 简体 translations: POMELO_REPLAY=<folder of runs>
    #[test]
    #[ignore]
    fn replay_stray_traditional() {
        let Ok(dir) = std::env::var("POMELO_REPLAY") else { return };
        let cc = OpenCC::from_config(BuiltinConfig::T2s).unwrap();
        let mut stack = vec![std::path::PathBuf::from(dir)];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                let p = e.path();
                let name = p.file_name().unwrap().to_string_lossy().to_string();
                if p.is_dir() && !name.contains("繁") && !name.contains("美國") {
                    stack.push(p);
                } else if name.starts_with("译文") && name.ends_with(".mmd") {
                    let text = std::fs::read_to_string(&p).unwrap();
                    let changed: Vec<String> = text.chars().zip(cc.convert(&text).chars()).filter(|(a, b)| a != b).map(|(a, b)| format!("{a}→{b}")).collect();
                    if !changed.is_empty() {
                        eprintln!("[replay] {}：{}", p.display(), changed.join(" "));
                    }
                }
            }
        }
    }
}
