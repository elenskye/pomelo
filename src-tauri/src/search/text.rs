//! Text rules of the index (bump `RULES` when any of them changes: everything is rebuilt).
//! - Words (FTS5, unicode61): CJK runs become overlapping two-character tokens plus the run's last
//!   character alone, so every character starts a token. A query word of two or more CJK
//!   characters is the phrase of its pairs (= the substring, like 全局搜索's instr); one character is
//!   a prefix; other runs (Latin, digits) are word prefixes ("habit" finds "Habits", not "abits").
//!   Every query word must match. Case and diacritics don't matter.
//! - Passages for vectors: picky::index::split (about 240 tokens, sentence aligned), at most
//!   MAX_PASSAGES per record; a later passage is prefixed with the record's title.
//! - Fingerprint: FNV-1a over everything stored for a doc (stable across builds).

pub const RULES: i64 = 1;
pub const MAX_WORDS: usize = 8;
pub const MAX_PASSAGES: usize = 64;

/// Chinese, Japanese, Korean letters: searched by character pairs, not by words.
pub fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF | 0x20000..=0x2FA1F)
}

enum Run {
    Cjk(Vec<char>),
    Word(String),
}

fn runs(text: &str) -> Vec<Run> {
    let mut out = vec![];
    let (mut cjk, mut word): (Vec<char>, String) = (vec![], String::new());
    for c in text.chars().flat_map(char::to_lowercase).chain([' ']) {
        let class = if is_cjk(c) { 1 } else if c.is_alphanumeric() { 2 } else { 0 };
        if class != 1 && !cjk.is_empty() {
            out.push(Run::Cjk(std::mem::take(&mut cjk)));
        }
        if class != 2 && !word.is_empty() {
            out.push(Run::Word(std::mem::take(&mut word)));
        }
        match class {
            1 => cjk.push(c),
            2 => word.push(c),
            _ => {}
        }
    }
    out
}

/// The token stream stored in docs_fts for `text`.
pub fn tokens(text: &str) -> String {
    let mut out: Vec<String> = vec![];
    for r in runs(text) {
        match r {
            Run::Cjk(cs) => {
                for i in 0..cs.len() {
                    out.push(cs[i..(i + 2).min(cs.len())].iter().collect());
                }
            }
            Run::Word(w) => out.push(w),
        }
    }
    out.join(" ")
}

/// An FTS5 MATCH expression requiring every word of `q`; None when it has nothing searchable.
pub fn fts_query(q: &str) -> Option<String> {
    let mut parts = vec![];
    for word in q.split_whitespace().take(MAX_WORDS) {
        for r in runs(word) {
            parts.push(match r {
                Run::Cjk(cs) if cs.len() == 1 => format!("\"{}\"*", cs[0]),
                Run::Cjk(cs) => format!("\"{}\"", cs.windows(2).map(|p| p.iter().collect::<String>()).collect::<Vec<_>>().join(" ")),
                Run::Word(w) => format!("\"{w}\"*"),
            });
        }
    }
    (!parts.is_empty()).then(|| parts.join(" AND "))
}

/// The first query word, lowercased (for snippets).
pub fn first_word(q: &str) -> String {
    q.split_whitespace().next().unwrap_or("").to_lowercase()
}

/// Passages a doc's vectors are made from.
pub fn passages(title: &str, body: &str) -> Vec<String> {
    let mut list = crate::picky::index::split(body);
    if list.is_empty() {
        list.push(title.to_string());
    }
    list.truncate(MAX_PASSAGES);
    list.into_iter().enumerate().map(|(i, p)| if i == 0 || p.contains(title) { p } else { format!("{title}\n{p}") }).collect()
}

/// FNV-1a, 64 bit, over the parts (separated so ("ab", "c") ≠ ("a", "bc")).
pub fn hash(parts: &[&str]) -> i64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for p in parts {
        for b in p.bytes().chain([0xff]) {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }
    h as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cjk_becomes_pairs_and_words_stay_words() {
        assert_eq!(tokens("我要找房子了"), "我要 要找 找房 房子 子了 了");
        assert_eq!(tokens("iPhone15价格，Café!"), "iphone15 价格 格 café");
        assert_eq!(tokens("房"), "房");
        assert_eq!(tokens("  。！ "), "");
    }

    #[test]
    fn queries_need_every_word() {
        assert_eq!(fts_query("找房子").as_deref(), Some("\"找房 房子\""));
        assert_eq!(fts_query("房 Atomic").as_deref(), Some("\"房\"* AND \"atomic\"*"));
        assert_eq!(fts_query("iPhone价格").as_deref(), Some("\"iphone\"* AND \"价格\""));
        assert_eq!(fts_query(" ，。 "), None);
    }

    #[test]
    fn matching_through_sqlite() {
        let c = rusqlite::Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE VIRTUAL TABLE t USING fts5(tokens, content='', contentless_delete=1, tokenize='unicode61 remove_diacritics 2');").unwrap();
        let docs = ["我在找房子，预算 4500", "租房。房子很旧", "Atomic Habits 读书笔记", "周末去公园散步"];
        for (i, d) in docs.iter().enumerate() {
            c.execute("INSERT INTO t (rowid, tokens) VALUES (?1, ?2)", rusqlite::params![i as i64 + 1, tokens(d)]).unwrap();
        }
        let hits = |q: &str| -> Vec<i64> {
            let mut st = c.prepare("SELECT rowid FROM t WHERE t MATCH ?1 ORDER BY rowid").unwrap();
            st.query_map([fts_query(q).unwrap()], |r| r.get(0)).unwrap().map(Result::unwrap).collect()
        };
        assert_eq!(hits("找房子"), [1]); // a substring, not 租房。房子
        assert_eq!(hits("房子"), [1, 2]);
        assert_eq!(hits("房"), [1, 2]);
        assert_eq!(hits("habit"), [3]);
        assert_eq!(hits("HABITS 笔记"), [3]);
        assert_eq!(hits("abits"), Vec::<i64>::new()); // words match from their start
        assert_eq!(hits("45"), [1]);
        c.execute("DELETE FROM t WHERE rowid = 1", []).unwrap();
        assert_eq!(hits("房子"), [2]);
    }

    #[test]
    fn passages_and_hashes() {
        assert_eq!(passages("标题", ""), ["标题"]);
        let long = "这是一句很长的话，用来把一条记录切成好几段。".repeat(40);
        let ps = passages("长笔记", &long);
        assert!(ps.len() > 1 && ps[1].starts_with("长笔记\n"));
        assert_ne!(hash(&["ab", "c"]), hash(&["a", "bc"]));
        assert_eq!(hash(&["x"]), hash(&["x"]));
    }
}
