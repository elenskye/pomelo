//! Vectors in memory: each passage vector is normalised and stored as int8 with one f32 scale
//! (1 KB for 1024 dimensions; ~200 MB at 200 000 passages). A search quantises the question
//! the same way and compares every allowed passage (integer dot products), keeping each doc's
//! best passage. `VectorIndex` is the part a smarter (approximate) index would replace.

use std::collections::{HashMap, HashSet};

/// A normalised vector as int8 values and the scale that turns them back into floats.
#[derive(Debug, Clone, PartialEq)]
pub struct Quantized {
    pub scale: f32,
    pub values: Vec<i8>,
}

pub fn quantize(v: &[f32]) -> Quantized {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    let max = v.iter().fold(0f32, |m, x| m.max(x.abs()));
    if norm == 0.0 || max == 0.0 {
        return Quantized { scale: 0.0, values: vec![0; v.len()] };
    }
    let scale = max / norm / 127.0;
    Quantized { scale, values: v.iter().map(|x| (x / norm / scale).round().clamp(-127.0, 127.0) as i8).collect() }
}

/// Stored form: scale (f32, little endian) then the values.
pub fn to_blob(q: &Quantized) -> Vec<u8> {
    q.scale.to_le_bytes().into_iter().chain(q.values.iter().map(|x| *x as u8)).collect()
}

pub fn from_blob(b: &[u8]) -> Option<Quantized> {
    let (head, rest) = (b.get(..4)?, b.get(4..)?);
    Some(Quantized { scale: f32::from_le_bytes(head.try_into().ok()?), values: rest.iter().map(|x| *x as i8).collect() })
}

/// One passage of a doc.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Entry {
    pub doc: i64,
    pub ordinal: u16,
    /// Index into sources::SOURCES (filters use one bit per source).
    pub source: u8,
}

/// A doc's best passage for a question.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hit {
    pub doc: i64,
    pub ordinal: u16,
    pub score: f32,
}

pub trait VectorIndex: Send + Sync {
    fn model(&self) -> &str;
    fn add(&mut self, e: Entry, q: &Quantized);
    fn remove(&mut self, docs: &HashSet<i64>);
    /// The `k` best docs among passages whose source bit is set in `allow`, scoring ≥ `min`.
    fn search(&self, question: &[f32], allow: u32, k: usize, min: f32) -> Vec<Hit>;
}

/// Compares the question with every passage.
pub struct Flat {
    model: String,
    dims: usize,
    entries: Vec<Entry>,
    scales: Vec<f32>,
    data: Vec<i8>,
}

impl Flat {
    pub fn new(model: &str) -> Self {
        Flat { model: model.into(), dims: 0, entries: vec![], scales: vec![], data: vec![] }
    }
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

pub fn dot(a: &[i8], b: &[i8]) -> i32 {
    a.iter().zip(b).map(|(x, y)| i32::from(*x) * i32::from(*y)).sum()
}

impl VectorIndex for Flat {
    fn model(&self) -> &str {
        &self.model
    }
    fn add(&mut self, e: Entry, q: &Quantized) {
        if self.dims == 0 {
            self.dims = q.values.len();
        }
        if q.values.len() != self.dims {
            return; // another model's vector: never compared
        }
        self.entries.push(e);
        self.scales.push(q.scale);
        self.data.extend_from_slice(&q.values);
    }
    fn remove(&mut self, docs: &HashSet<i64>) {
        if docs.is_empty() {
            return;
        }
        let mut keep = 0;
        for i in 0..self.entries.len() {
            if docs.contains(&self.entries[i].doc) {
                continue;
            }
            if keep != i {
                self.entries[keep] = self.entries[i];
                self.scales[keep] = self.scales[i];
                self.data.copy_within(i * self.dims..(i + 1) * self.dims, keep * self.dims);
            }
            keep += 1;
        }
        self.entries.truncate(keep);
        self.scales.truncate(keep);
        self.data.truncate(keep * self.dims);
    }
    fn search(&self, question: &[f32], allow: u32, k: usize, min: f32) -> Vec<Hit> {
        if self.entries.is_empty() || question.len() != self.dims || k == 0 {
            return vec![];
        }
        let q = quantize(question);
        let mut scored: Vec<(f32, usize)> = Vec::with_capacity(self.entries.len());
        for (i, e) in self.entries.iter().enumerate() {
            if allow & (1 << e.source) == 0 {
                continue;
            }
            let s = dot(&q.values, &self.data[i * self.dims..(i + 1) * self.dims]) as f32 * q.scale * self.scales[i];
            if s >= min {
                scored.push((s, i));
            }
        }
        // the best passages (a doc may have several), then each doc's best
        let take = (k * 4).min(scored.len());
        if take < scored.len() {
            scored.select_nth_unstable_by(take, |a, b| b.0.total_cmp(&a.0));
            scored.truncate(take);
        }
        let mut best: HashMap<i64, Hit> = HashMap::new();
        for (s, i) in scored {
            let e = self.entries[i];
            let h = best.entry(e.doc).or_insert(Hit { doc: e.doc, ordinal: e.ordinal, score: s });
            if s > h.score {
                *h = Hit { doc: e.doc, ordinal: e.ordinal, score: s };
            }
        }
        let mut out: Vec<Hit> = best.into_values().collect();
        out.sort_by(|a, b| b.score.total_cmp(&a.score).then(b.doc.cmp(&a.doc)));
        out.truncate(k);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cos(a: &[f32], b: &[f32]) -> f32 {
        crate::picky::embed::cosine(a, b)
    }

    #[test]
    fn int8_keeps_the_ranking() {
        let a: Vec<f32> = (0..256).map(|i| ((i * 7 % 13) as f32 - 6.0) / 3.0).collect();
        let b: Vec<f32> = (0..256).map(|i| ((i * 5 % 11) as f32 - 5.0) / 2.0).collect();
        let (qa, qb) = (quantize(&a), quantize(&b));
        let approx = dot(&qa.values, &qb.values) as f32 * qa.scale * qb.scale;
        assert!((approx - cos(&a, &b)).abs() < 0.01, "{approx} vs {}", cos(&a, &b));
        assert_eq!(from_blob(&to_blob(&qa)).unwrap(), qa);
        assert_eq!(quantize(&[0.0, 0.0]).scale, 0.0);
    }

    #[test]
    fn best_passage_per_doc_within_allowed_sources() {
        let mv = crate::picky::embed::mock_vector;
        let mut f = Flat::new("m");
        let add = |f: &mut Flat, doc: i64, ordinal: u16, source: u8, text: &str| f.add(Entry { doc, ordinal, source }, &quantize(&mv(text)));
        add(&mut f, 1, 0, 0, "周末去公园散步");
        add(&mut f, 1, 1, 0, "整理租房合同和押金");
        add(&mut f, 2, 0, 1, "租房合同的押金怎么退");
        add(&mut f, 3, 0, 0, "读完一本小说");
        let q = mv("租房合同的押金怎么退");
        let hits = f.search(&q, u32::MAX, 10, 0.2);
        assert_eq!(hits.iter().map(|h| h.doc).collect::<Vec<_>>(), [2, 1]);
        assert_eq!(hits[1].ordinal, 1); // doc 1's best passage
        assert_eq!(f.search(&q, 1 << 0, 10, 0.2).iter().map(|h| h.doc).collect::<Vec<_>>(), [1]); // source 1 not allowed
        f.remove(&HashSet::from([1]));
        assert_eq!((f.len(), f.search(&q, u32::MAX, 10, 0.2).len()), (2, 1));
        f.add(Entry { doc: 9, ordinal: 0, source: 0 }, &quantize(&[1.0, 2.0])); // other dimensions: ignored
        assert_eq!(f.len(), 2);
    }
}
