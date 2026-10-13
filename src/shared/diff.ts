// A small text diff for showing what 润色 changed (no library): longest common subsequence over tokens —
// one token per Chinese character / punctuation, words and runs of spaces kept whole. Long texts fall back
// to sentences (and lines) so the table stays small.
export type DiffPart = { kind: "same" | "del" | "add"; text: string };

const CELLS = 2_500_000;

const charTokens = (s: string) => s.match(/[A-Za-z0-9_'’-]+|\s+|[\s\S]/gu) ?? [];
const sentenceTokens = (s: string) => s.match(/[^。！？!?；;\n]*[。！？!?；;\n]?/gu)?.filter(Boolean) ?? [];
const lineTokens = (s: string) => s.match(/[^\n]*\n?/gu)?.filter(Boolean) ?? [];

function lcs(x: string[], y: string[]): DiffPart[] {
  const n = x.length;
  const m = y.length;
  const w = m + 1;
  const t = new Uint16Array((n + 1) * w);
  for (let i = n - 1; i >= 0; i--)
    for (let j = m - 1; j >= 0; j--) t[i * w + j] = x[i] === y[j] ? t[(i + 1) * w + j + 1] + 1 : Math.max(t[(i + 1) * w + j], t[i * w + j + 1]);
  const out: DiffPart[] = [];
  const push = (kind: DiffPart["kind"], text: string) => {
    const last = out[out.length - 1];
    if (last?.kind === kind) last.text += text;
    else out.push({ kind, text });
  };
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (x[i] === y[j]) {
      push("same", x[i]);
      i++;
      j++;
    } else if (t[(i + 1) * w + j] >= t[i * w + j + 1]) push("del", x[i++]);
    else push("add", y[j++]);
  }
  while (i < n) push("del", x[i++]);
  while (j < m) push("add", y[j++]);
  return out;
}

export function diff(a: string, b: string): DiffPart[] {
  for (const split of [charTokens, sentenceTokens, lineTokens]) {
    const x = split(a);
    const y = split(b);
    if ((x.length + 1) * (y.length + 1) <= CELLS && x.length < 65000 && y.length < 65000) return lcs(x, y);
  }
  return [
    ...(a ? [{ kind: "del" as const, text: a }] : []),
    ...(b ? [{ kind: "add" as const, text: b }] : []),
  ];
}
