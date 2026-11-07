/* @ds-bundle: {"format":4,"namespace":"Pomelo","components":[]} */
/* Pomelo 柚儿园：手画零件。由 tooling/design-system/bundle.mjs 把 line.mjs、frame.mjs、sticker.mjs、paint.mjs 原样打包，不要手改。 */
(function () {
'use strict';
// ---- tooling/handmade/line.mjs ----
// 手绘的线：抖动算进路径，不靠运行时的 SVG 滤镜。算法从 Pomelo 的 Phase 2 小样搬来（站主 2026-11-05 验收），数值都由调用方给。
// 手绘感：沿路径均匀取点，按平滑噪声沿法线方向偏移，再用 Catmull-Rom 连成曲线。
// 同一个 seed 永远得到同一条线，两次生成结果一致。

const f = (n) => { const v = Math.round(n * 10) / 10; return Object.is(v, -0) ? '0' : String(v); };
const f1 = f;

function rng(seed) {
  let a = (seed * 2654435761) >>> 0 || 1;
  return () => {
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

// 不闭合的平滑噪声，值大约在 -1..1
function noise(seed) {
  const r = rng(seed);
  const cs = [1, 2.1, 3.7].map((k, i) => ({ k: k * (0.8 + 0.4 * r()), p: r() * 6.283, a: [1, 0.45, 0.2][i] }));
  return (x) => cs.reduce((s, c) => s + c.a * Math.sin(c.k * x + c.p), 0) / 1.65;
}

// 闭合的平滑噪声：整数频率，绕一圈首尾相接
function pnoise(seed) {
  const r = rng(seed);
  const cs = [2, 3, 5].map((k, i) => ({ k, p: r() * 6.283, a: [1, 0.55, 0.3][i] }));
  return (u) => cs.reduce((s, c) => s + c.a * Math.sin(c.k * u * 6.283 + c.p), 0) / 1.85;
}

// ---------- 路径命令 ----------
// cmds: [['M',x,y],['L',x,y],['C',x1,y1,x2,y2,x,y],['Q',x1,y1,x,y],['Z']]
const K = 0.5523;
function ellipse(cx, cy, rx, ry, rot = 0) {
  const pts = [
    ['M', cx + rx, cy],
    ['C', cx + rx, cy + ry * K, cx + rx * K, cy + ry, cx, cy + ry],
    ['C', cx - rx * K, cy + ry, cx - rx, cy + ry * K, cx - rx, cy],
    ['C', cx - rx, cy - ry * K, cx - rx * K, cy - ry, cx, cy - ry],
    ['C', cx + rx * K, cy - ry, cx + rx, cy - ry * K, cx + rx, cy],
    ['Z'],
  ];
  return rot ? rotate(pts, cx, cy, rot) : pts;
}
const circle = (cx, cy, r) => ellipse(cx, cy, r, r);
function rrect(x, y, w, h, r) {
  return [
    ['M', x + r, y], ['L', x + w - r, y], ['Q', x + w, y, x + w, y + r],
    ['L', x + w, y + h - r], ['Q', x + w, y + h, x + w - r, y + h],
    ['L', x + r, y + h], ['Q', x, y + h, x, y + h - r],
    ['L', x, y + r], ['Q', x, y, x + r, y], ['Z'],
  ];
}
function mapCmds(cmds, fn) {
  return cmds.map((c) => {
    if (c[0] === 'Z') return c;
    const out = [c[0]];
    for (let i = 1; i < c.length; i += 2) { const [x, y] = fn(c[i], c[i + 1]); out.push(x, y); }
    return out;
  });
}
function rotate(cmds, cx, cy, deg) {
  const a = (deg * Math.PI) / 180, co = Math.cos(a), si = Math.sin(a);
  return mapCmds(cmds, (x, y) => [cx + (x - cx) * co - (y - cy) * si, cy + (x - cx) * si + (y - cy) * co]);
}
const move = (cmds, dx, dy) => mapCmds(cmds, (x, y) => [x + dx, y + dy]);
const scale = (cmds, s, ox = 0, oy = 0) => mapCmds(cmds, (x, y) => [ox + (x - ox) * s, oy + (y - oy) * s]);
// 'M10 20 L30 40 C… Q… Z' 这样的简单字符串（只用绝对的 M L C Q Z）
function P(str) {
  const tok = str.match(/[MLCQZ]|-?\d*\.?\d+/g);
  const out = []; let i = 0;
  const n = { M: 2, L: 2, C: 6, Q: 4, Z: 0 };
  while (i < tok.length) {
    const t = tok[i++];
    const args = tok.slice(i, i + n[t]).map(Number); i += n[t];
    out.push([t, ...args]);
  }
  return out;
}
const cmdsToD = (cmds) => cmds.map((c) => c[0] + c.slice(1).map(f).join(' ')).join('');

// 把命令拆成若干段折线
function sample(cmds, step = 0.6) {
  const subs = []; let cur = null, x = 0, y = 0, sx = 0, sy = 0;
  const line = (x2, y2) => {
    const n = Math.max(1, Math.ceil(Math.hypot(x2 - x, y2 - y) / step));
    for (let i = 1; i <= n; i++) cur.pts.push([x + ((x2 - x) * i) / n, y + ((y2 - y) * i) / n]);
    x = x2; y = y2;
  };
  for (const c of cmds) {
    if (c[0] === 'M') { cur = { pts: [[c[1], c[2]]], closed: false }; subs.push(cur); x = sx = c[1]; y = sy = c[2]; }
    else if (c[0] === 'L') line(c[1], c[2]);
    else if (c[0] === 'C' || c[0] === 'Q') {
      const p = c[0] === 'Q'
        ? [x, y, x + (2 / 3) * (c[1] - x), y + (2 / 3) * (c[2] - y), c[3] + (2 / 3) * (c[1] - c[3]), c[4] + (2 / 3) * (c[2] - c[4]), c[3], c[4]]
        : [x, y, ...c.slice(1)];
      const len = Math.hypot(p[2] - p[0], p[3] - p[1]) + Math.hypot(p[4] - p[2], p[5] - p[3]) + Math.hypot(p[6] - p[4], p[7] - p[5]);
      const n = Math.max(4, Math.ceil(len / step));
      for (let i = 1; i <= n; i++) {
        const t = i / n, u = 1 - t;
        cur.pts.push([
          u * u * u * p[0] + 3 * u * u * t * p[2] + 3 * u * t * t * p[4] + t * t * t * p[6],
          u * u * u * p[1] + 3 * u * u * t * p[3] + 3 * u * t * t * p[5] + t * t * t * p[7],
        ]);
      }
      x = p[6]; y = p[7];
    } else if (c[0] === 'Z') { if (Math.hypot(x - sx, y - sy) > 1e-6) line(sx, sy); cur.closed = true; }
  }
  return subs;
}

function resample(pts, sp) {
  const out = [pts[0]]; let acc = 0;
  for (let i = 1; i < pts.length; i++) {
    let [ax, ay] = pts[i - 1]; const [bx, by] = pts[i];
    let seg = Math.hypot(bx - ax, by - ay);
    while (acc + seg >= sp) {
      const t = (sp - acc) / seg;
      ax += (bx - ax) * t; ay += (by - ay) * t;
      out.push([ax, ay]); seg = Math.hypot(bx - ax, by - ay); acc = 0;
    }
    acc += seg;
  }
  const last = pts[pts.length - 1], tail = out[out.length - 1];
  if (Math.hypot(last[0] - tail[0], last[1] - tail[1]) > sp * 0.3) out.push(last);
  return out;
}

const fi = (n) => { const v = Math.round(n); return Object.is(v, -0) ? '0' : String(v); };
function cr(p, closed, prec = 1) {
  const f = prec ? f1 : fi;
  const n = p.length;
  if (n < 2) return '';
  const at = (i) => (closed ? p[(i + n) % n] : p[Math.max(0, Math.min(n - 1, i))]);
  let d = `M${f(p[0][0])} ${f(p[0][1])}`;
  const last = closed ? n : n - 1;
  for (let i = 0; i < last; i++) {
    const p0 = at(i - 1), p1 = at(i), p2 = at(i + 1), p3 = at(i + 2);
    d += `C${f(p1[0] + (p2[0] - p0[0]) / 6)} ${f(p1[1] + (p2[1] - p0[1]) / 6)} ${f(p2[0] - (p3[0] - p1[0]) / 6)} ${f(p2[1] - (p3[1] - p1[1]) / 6)} ${f(p2[0])} ${f(p2[1])}`;
  }
  return d + (closed ? 'Z' : '');
}

function normalAt(seq, i) {
  const a = seq[Math.max(0, i - 1)], b = seq[Math.min(seq.length - 1, i + 1)];
  let nx = -(b[1] - a[1]), ny = b[0] - a[0];
  const l = Math.hypot(nx, ny) || 1;
  return [nx / l, ny / l];
}

// 手画的描边：闭合图形从随机处起笔，多画一点再收，首尾不完全对齐
function handStroke(cmds, { amp = 0.8, wave = 9, seed = 1, over = 0.06, drift = 0.9, sp = 0.9, keep = 3, prec = 1 } = {}) {
  return sample(cmds).map((sub, k) => {
    const pts = resample(sub.pts, sp);
    if (pts.length < 2) return '';
    const nz = noise(seed * 31 + k);
    let seq = pts;
    if (sub.closed) {
      const N = pts.length - 1; // 最后一点和第一点重合
      const start = Math.floor(rng(seed * 7 + k)() * N);
      const extra = Math.round(N * over);
      seq = [];
      for (let i = 0; i <= N + extra; i++) seq.push(pts[(start + i) % N]);
    }
    const out = seq.map((p, i) => {
      const [nx, ny] = normalAt(seq, i);
      const off = amp * nz((i * sp) / wave) + (sub.closed ? drift * amp * (i / seq.length - 0.5) : 0);
      return [p[0] + nx * off, p[1] + ny * off];
    });
    const dec = out.filter((_, i) => i % keep === 0 || i === out.length - 1);
    return cr(dec, false, prec);
  }).join('');
}

// 手剪的填色块：闭合、边缘轻微起伏
function handFill(cmds, { amp = 0.5, seed = 1, sp = 0.9, keep = 3, prec = 1 } = {}) {
  return sample(cmds).map((sub, k) => {
    let pts = resample(sub.pts, sp);
    if (!sub.closed) return cr(pts.filter((_, i) => i % keep === 0), false, prec);
    pts = pts.slice(0, -1);
    const N = pts.length;
    const nz = pnoise(seed * 13 + k);
    const out = pts.map((p, i) => {
      const [nx, ny] = normalAt([pts[(i - 1 + N) % N], p, pts[(i + 1) % N]], 1);
      const off = amp * nz(i / N);
      return [p[0] + nx * off, p[1] + ny * off];
    });
    return cr(out.filter((_, i) => i % keep === 0), true, prec);
  }).join('');
}

function mix(a, b, t) {
  const pa = a.match(/\w\w/g).map((h) => parseInt(h, 16));
  const pb = b.match(/\w\w/g).map((h) => parseInt(h, 16));
  return '#' + pa.map((v, i) => Math.round(v + (pb[i] - v) * t).toString(16).padStart(2, '0')).join('');
}

// ---- tooling/handmade/frame.mjs ----
// 手作零件：手画的框、线、荧光笔、划掉、胶带、缝线。都返回 HTML / SVG 字符串，颜色默认走 CSS 变量。
// 数值的默认值是 Pomelo 验收过的样子，正式用的时候由调用方按 tokens/handmade.json 传。
// import { P, rrect, handStroke, handFill, f, rng } from './line.mjs';

// 两种胶带的条纹色（CSS 变量），调用方也可以传具体颜色
const TAPE_VARS = [['var(--tape-a0)', 'var(--tape-b0)'], ['var(--tape-a1)', 'var(--tape-b1)']];

function wob(w, h, { r = 14, amp = 0.9, sw = 1.8, color = 'var(--pen)', seed = 1, fill = null, dx = 0, dy = 0, pad = 8, stroke = true, cls = '', open = null } = {}) {
  const shape = rrect(pad, pad, w, h, r);
  const per = 2 * (w + h);
  const sp = 3, keep = per > 2400 ? 3 : 2;
  const fillPath = fill ? `<path d="${handFill(shape, { amp: amp * 0.8, seed, sp, keep: keep + 1, prec: 0 })}" fill="${fill}" transform="translate(${dx} ${dy})"></path>` : '';
  let strokePath = '';
  if (stroke) {
    const shp = open === 'left' ? P(`M${pad + r} ${pad} L${pad + w - r} ${pad} Q${pad + w} ${pad} ${pad + w} ${pad + r} L${pad + w} ${pad + h - r} Q${pad + w} ${pad + h} ${pad + w - r} ${pad + h} L${pad + r} ${pad + h}`) : shape;
    strokePath = `<path d="${handStroke(shp, { amp, wave: 26, seed, sp, keep, over: 0.03, drift: 1.2, prec: w > 60 && h > 30 ? 0 : 1 })}" fill="none" stroke="${color}" stroke-width="${sw}" stroke-linecap="round" stroke-linejoin="round"></path>`;
  }
  return `<svg class="wob ${cls}" width="${w + pad * 2 + Math.abs(dx)}" height="${h + pad * 2 + Math.abs(dy)}" style="position:absolute;left:-${pad}px;top:-${pad}px;pointer-events:none;overflow:visible" aria-hidden="true">${fillPath}${strokePath}</svg>`;
}

// 一条手画的线（横线、下划线、分割线）
function hline(w, { seed = 1, sw = 1.6, color = 'var(--pen)', amp = 0.8, y = 4, cls = '', style = '' } = {}) {
  const d = handStroke(P(`M2 ${y} C${w * 0.33} ${y - 1.2} ${w * 0.66} ${y + 1.2} ${w - 2} ${y}`), { amp, wave: 22, seed, sp: 2, keep: 3, prec: w > 200 ? 0 : 1 });
  return `<svg class="${cls}" width="${w}" height="${y * 2}" style="display:block;overflow:visible;${style}" aria-hidden="true"><path pathLength="1" d="${d}" fill="none" stroke="${color}" stroke-width="${sw}" stroke-linecap="round"></path></svg>`;
}
function vline(h, { seed = 1, sw = 1.4, color = 'var(--line)', amp = 0.8, cls = '', style = '' } = {}) {
  const d = handStroke(P(`M4 2 C3 ${h * 0.33} 5 ${h * 0.66} 4 ${h - 2}`), { amp, wave: 26, seed, sp: 3, keep: 3, prec: 0 });
  return `<svg class="${cls}" width="8" height="${h}" style="display:block;overflow:visible;${style}" aria-hidden="true"><path d="${d}" fill="none" stroke="${color}" stroke-width="${sw}" stroke-linecap="round"></path></svg>`;
}

// 荧光笔划过（放在文字下面）
function marker(w, { seed = 5, sw = 12, color = 'var(--marker)', cls = '', top = '55%', opacity = 0.9 } = {}) {
  const d = handStroke(P(`M4 12 C${w * 0.33} 10 ${w * 0.66} 14 ${w - 4} 11`), { amp: 1.2, wave: 20, seed, sp: 2, keep: 3 });
  return `<svg class="mk ${cls}" width="${w}" height="24" style="position:absolute;left:-4px;top:${top};margin-top:-12px;z-index:-1;overflow:visible;pointer-events:none" aria-hidden="true"><path pathLength="1" d="${d}" stroke="${color}" stroke-width="${sw}" stroke-linecap="round" fill="none" opacity="${opacity}"></path></svg>`;
}
// 划掉
function strike(w, { seed = 9, sw = 2, color = 'var(--ink2)', cls = '' } = {}) {
  const d = handStroke(P(`M0 6 C${w * 0.3} 4 ${w * 0.7} 8 ${w} 5`), { amp: 0.8, wave: 14, seed, sp: 1.5, keep: 3 });
  return `<svg class="sk ${cls}" width="${w + 8}" height="12" style="position:absolute;left:-4px;top:50%;margin-top:-6px;overflow:visible;pointer-events:none" aria-hidden="true"><path pathLength="1" d="${d}" transform="translate(4 0)" stroke="${color}" stroke-width="${sw}" stroke-linecap="round" fill="none"></path></svg>`;
}

// 胶带：两种（薄荷条纹、柚黄条纹），锯齿边
function zig(n = 5, d = 5) {
  const L1 = [], R1 = [];
  for (let i = 0; i <= n * 2; i++) {
    const y = (i / (n * 2)) * 100;
    L1.push(`${i % 2 ? d : 0}% ${f(y)}%`);
    R1.unshift(`${i % 2 ? 100 - d : 100}% ${f(y)}%`);
  }
  return `polygon(${[...L1, ...R1].join(',')})`;
}
const ZIG = zig();
function tape({ x, y, w = 88, h = 22, rot = -4, kind = 0, z = 4, cls = '', colors = TAPE_VARS }) {
  const [a, b] = colors[kind];
  return `<div class="tp ${cls}" style="position:absolute;left:${x}px;top:${y}px;width:${w}px;height:${h}px;background:repeating-linear-gradient(45deg,${a} 0 7px,${b} 7px 14px);opacity:.88;clip-path:${ZIG};transform:rotate(${rot}deg);z-index:${z};pointer-events:none"></div>`;
}
// 每张卡片随机一到两条胶带（种子固定，两次生成一样）
function tapes(w, h, seed, n = 'rand', colors = TAPE_VARS) {
  const r0 = rng(seed * 17 + 3);
  const nT = n === 'rand' ? (r0() < 0.5 ? 1 : 2) : n;
  const second = [{ x: w - 64, y: h - 14, w: 76, rot: 32 }, { x: -18, y: 4, w: 70, rot: -38 }, { x: w - 54, y: -4, w: 72, rot: 36 }][Math.floor(r0() * 3)];
  const first = { x: Math.round(w / 2 - 44 + (r0() - 0.5) * 40), y: -11, rot: -4 + Math.round((r0() - 0.5) * 6) };
  return [tape({ ...first, colors }), tape({ ...second, kind: 1, colors })].slice(0, nT).join('');
}

// 缝线：纸左边一列手缝的短线
function stitches(h, { x = 9, gap = 20, len = 9, seed = 3, color = 'var(--pen)', opacity = 0.55 } = {}) {
  const r = rng(seed);
  let d = '';
  for (let y = 16; y < h - 16; y += gap) {
    const tilt = (r() - 0.5) * 2.4;
    d += handStroke(P(`M${x - tilt / 2} ${y} L${x + tilt / 2} ${y + len}`), { amp: 0.35, wave: 6, seed: y, sp: 1, keep: 2 });
  }
  return `<svg width="${x * 2}" height="${h}" style="position:absolute;left:0;top:0;pointer-events:none;opacity:${opacity}" aria-hidden="true"><path d="${d}" fill="none" stroke="${color}" stroke-width="2" stroke-linecap="round"></path></svg>`;
}

// 胶带的 SVG 版：单独出图时用（HTML 里用上面的 tape）
function tapeSvg(x, y, w, h, rot, [a, b], { id = 'tp', stripe = 7, opacity = 0.88, teeth = 5, depth = 5 } = {}) {
  const pts = [];
  const right = [];
  for (let i = 0; i <= teeth * 2; i++) {
    const py = (i / (teeth * 2)) * h;
    const d = i % 2 ? (depth / 100) * w : 0;
    pts.push(`${f(d)},${f(py)}`);
    right.unshift(`${f(w - d)},${f(py)}`);
  }
  return `<defs><pattern id="${id}" width="${stripe * 2}" height="${stripe * 2}" patternUnits="userSpaceOnUse" patternTransform="rotate(45)"><rect width="${stripe * 2}" height="${stripe * 2}" fill="${a}"></rect><rect width="${stripe}" height="${stripe * 2}" fill="${b}"></rect></pattern></defs><polygon points="${[...pts, ...right].join(' ')}" fill="url(#${id})" opacity="${opacity}" transform="translate(${x} ${y}) rotate(${rot} ${f(w / 2)} ${f(h / 2)})"></polygon>`;
}

// ---- tooling/handmade/sticker.mjs ----
// 贴纸和错版：一张图由若干部分（art.parts）组成，每部分是图形（shape）、线（line）、点（dot）或腮红（blush）。
//   白边 wb：所有部分描粗铺一层，外面再加投影；填色 fl：比墨线错开 off；墨线 ol：手画。
// st 是画法参数（tokens/handmade.json 的 sticker），col 是颜色表（可以是 CSS 变量）：
//   { ink, white, shadow, blush, cats: { 分类: 色 }, catLight(分类), 其他名字: 色 }。图的 fill 写 'cat' 或 col 里的名字。
// import { handStroke, handFill, f, mix } from './line.mjs';

function render(art, st, col, { k = 1, merge = true, idp = 'a', anim = false, over = {}, layer = null } = {}) {
  const s = { ...st, ...over };
  const C = (name) => {
    if (name === 'cat') return col.cats[art.cat];
    if (name === 'catLight') return col.catLight ? col.catLight(art.cat) : mix(col.cats[art.cat], '#ffffff', 0.72);
    return col[name] ?? name;
  };
  const off = s.off.map((v) => v * k);
  const bw = s.border * k;
  const defs = s.shadow && bw > 0
    ? `<filter id="${idp}sh" x="-30%" y="-30%" width="160%" height="170%"><feDropShadow dx="0" dy="${f(1.3 * k)}" stdDeviation="${f(1.2 * k)}" flood-color="${col.shadow}" flood-opacity="1"></feDropShadow></filter>`
    : '';
  const shadowAttr = defs ? ` filter="url(#${idp}sh)"` : '';
  const blob = (p) => {
    const extra = Math.max(Math.abs(off[0]), Math.abs(off[1])) * 0.6;
    const w = 2 * (bw + extra);
    if (p.t === 'line') return `<path d="${handStroke(p.d, { amp: s.amp * k * 0.6, seed: p.seed, sp: s.sp ?? 0.9 })}" fill="none" stroke="${col.white}" stroke-width="${f(s.line * k * (p.sw ?? 0.8) + w)}" stroke-linecap="round" stroke-linejoin="round"></path>`;
    if (p.t === 'blush') return '';
    return `<path d="${handFill(p.d, { amp: s.fillAmp * k, seed: p.seed, sp: s.sp ?? 0.9 })}" fill="${col.white}" stroke="${col.white}" stroke-width="${f(w)}" stroke-linejoin="round"></path>`;
  };
  const body = (p) => {
    const amp = s.amp * k * (p.amp ?? 1);
    const tr = `transform="translate(${f(off[0])} ${f(off[1])})"`;
    const out = [];
    if (p.t === 'shape') {
      out.push(`<path class="fl" d="${handFill(p.d, { amp: s.fillAmp * k * (p.amp ?? 1), seed: p.seed, sp: s.sp ?? 0.9 })}" fill="${C(p.fill)}" ${s.outline ? tr : ''}></path>`);
      if (s.outline) out.push(`<path class="ol"${anim ? ' pathLength="1"' : ''} d="${handStroke(p.d, { amp, seed: p.seed, sp: s.sp ?? 0.9 })}" fill="none" stroke="${col.ink}" stroke-width="${f(s.line * k * (p.sw ?? 1))}" stroke-linecap="round" stroke-linejoin="round"></path>`);
    } else if (p.t === 'line') {
      const color = p.color ? C(p.color) : s.outline ? col.ink : C(p.sticker ?? 'white');
      const move = p.withFill && s.outline ? tr : '';
      out.push(`<path class="ol"${anim ? ' pathLength="1"' : ''} d="${handStroke(p.d, { amp: amp * 0.7, seed: p.seed, sp: s.sp ?? 0.9 })}" fill="none" stroke="${color}" stroke-width="${f(s.line * k * (p.sw ?? 0.8))}" stroke-linecap="round" stroke-linejoin="round" ${move}></path>`);
    } else if (p.t === 'dot') {
      out.push(`<path class="dt" d="${handFill(p.d, { amp: 0.12 * k, seed: p.seed })}" fill="${p.color ? C(p.color) : col.ink}"></path>`);
    } else if (p.t === 'blush') {
      out.push(`<path class="bl" d="${handFill(p.d, { amp: 0.3 * k, seed: p.seed })}" fill="${col.blush}" ${s.outline ? tr : ''}></path>`);
    }
    return out.join('');
  };
  let html = '';
  // 只要白边，或只要图形（柚子的脸要分层：白边在身子下面，五官在身子上面）
  if (layer === 'blob') return { defs, html: bw > 0 ? `<g class="wb"${shadowAttr}>${art.parts.map(blob).join('')}</g>` : '' };
  if (layer === 'body') return { defs, html: art.parts.map((p) => `<g class="pt${p.id ? ' ' + p.id : ''}">${body(p)}</g>`).join('') };
  if (merge) {
    if (bw > 0) html += `<g class="wb"${shadowAttr}>${art.parts.map(blob).join('')}</g>`;
    html += art.parts.map((p) => `<g class="pt${p.id ? ' ' + p.id : ''}">${body(p)}</g>`).join('');
  } else {
    // 按 id 分组：同一组的各块共用一张白边
    const groups = [];
    for (const p of art.parts) {
      const g = groups.find((x) => x.id && x.id === p.id);
      if (g) g.parts.push(p); else groups.push({ id: p.id, parts: [p] });
    }
    html += groups.map((g) => `<g class="pt${g.id ? ' ' + g.id : ''}">${bw > 0 ? `<g class="wb"${shadowAttr}>${g.parts.map(blob).join('')}</g>` : ''}${g.parts.map(body).join('')}</g>`).join('');
  }
  return { defs, html };
}

function svg(art, st, col, { size = 64, view = '-8 -8 80 80', k = 1, idp = 'a', merge = true, anim = false, cls = '', style = '', over = {}, label = '' } = {}) {
  const { defs, html } = render(art, st, col, { k, merge, idp, anim, over });
  const [, , vw, vh] = view.split(' ').map(Number);
  const h = Math.round((size * vh) / vw);
  return `<svg class="${cls}" width="${size}" height="${h}" viewBox="${view}" style="${style}" ${label ? `role="img" aria-label="${label}"` : 'aria-hidden="true"'}>${defs ? `<defs>${defs}</defs>` : ''}${html}</svg>`;
}

// ---- tooling/handmade/paint.mjs ----
// 手作零件的组件形状，和在页面上照 data-hand 把它们画出来的办法。
//   形状（纯函数，Node 和浏览器都能用，返回 SVG 字符串）：勾选框、评分星星、纸星星、开关、界面小图标、贴纸。
//   paint(root, opts)：只在浏览器里用。找 root 里带 data-hand 的元素，按它的宽高画手画的框、线、笔迹、胶带、缝线和上面这些形状。
// 几何取自 Pomelo 的 Phase 2 小样（站主 2026-11-05 验收：canvases/pomelo-explore/p2-final/ 的 parts.mjs、controls.mjs、zine.mjs）。
// 数值由调用方按方案的 tokens/handmade.json 传（params）；颜色由调用方给「角色 → CSS 值」的函数（color）和认得的角色（roles）。
// 画出来的东西都放在绝对定位的一层里（带 data-hand-painted），不改元素自己的大小；同一个宽高、同一个种子永远画出同一条线。
// Design System 的 components/bundle.js 由 tooling/design-system/bundle.mjs 把 line.mjs、frame.mjs、sticker.mjs 和本文件原样打包。
// import { P, circle, rrect, handStroke, handFill, f } from './line.mjs';
// import { wob, hline, marker, strike, tapes, stitches } from './frame.mjs';
// import { render } from './sticker.mjs';

const roleVar = (role) => `var(--color-${role})`;

// ---------- 形状 ----------

// 勾选框：线描贴纸，勾上时柚黄填色和勾才显示（由样式按 aria-checked 或 .is-on 决定）
const TICK = P('M6.4 12.6 L10.4 16.6 L17.6 7.4');
function checkboxSvg({ seed = 1, size = 22, c = roleVar } = {}) {
  const box = rrect(4, 4, 16, 16, 4.5);
  return `<svg class="pm-ck" width="${size}" height="${size}" viewBox="0 0 24 24" style="display:block;overflow:visible" aria-hidden="true"><g class="pm-ck-box"><path d="${handFill(box, { amp: 0.3, seed })}" fill="${c('art-white')}" stroke="${c('art-white')}" stroke-width="3"></path><g class="pm-ck-fill"><path d="${handFill(box, { amp: 0.3, seed: seed + 3 })}" fill="${c('yuzu')}" transform="translate(.8 1.1)"></path></g><path d="${handStroke(box, { amp: 0.38, seed, wave: 7 })}" fill="none" stroke="${c('pen')}" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round"></path><path class="pm-ck-tick" pathLength="1" d="${handStroke(TICK, { amp: 0.3, seed: seed + 1 })}" stroke="${c('art-ink')}" stroke-width="2.1" fill="none" stroke-linecap="round" stroke-linejoin="round"></path></g></svg>`;
}

// 评分的星星：柚黄填色错开一点，笔色描边；没选中时填色淡到 .12（样式决定）
const RATE_STAR = (() => {
  const pts = [];
  for (let k = 0; k < 10; k++) {
    const a = -Math.PI / 2 + (k * Math.PI) / 5, r = k % 2 ? 4.6 : 10.5;
    pts.push(`${f(12 + r * Math.cos(a))} ${f(12.5 + r * Math.sin(a))}`);
  }
  return P('M' + pts.join(' L') + ' Z');
})();
function ratingStarSvg({ seed = 1, size = 30, c = roleVar } = {}) {
  return `<svg class="pm-star" width="${size}" height="${size}" viewBox="0 0 24 24" style="display:block;overflow:visible" aria-hidden="true"><path class="pm-star-fill" d="${handFill(RATE_STAR, { amp: 0.25, seed })}" fill="${c('yuzu')}" transform="translate(.6 .8)"></path><path d="${handStroke(RATE_STAR, { amp: 0.3, seed: seed + 10, wave: 6 })}" fill="none" stroke="${c('pen')}" stroke-width="1.6" stroke-linejoin="round"></path></svg>`;
}

// 纸星星（幸运星）：鼓鼓的五角星，填分类色的贴纸色；金星是柚黄、描墨线
function puffyStar(r, k) {
  const pts = [];
  for (let i = 0; i < 10; i++) {
    const a = -Math.PI / 2 + (i * Math.PI) / 5, rr = i % 2 ? r * k : r;
    pts.push(`${f(Math.cos(a) * rr)} ${f(Math.sin(a) * rr)}`);
  }
  return P('M' + pts.join(' L') + ' Z');
}
function paperStarSvg({ seed = 1, size = 20, gold = false, fill = null, stroke = null, c = roleVar } = {}) {
  const head = `<svg class="pm-pstar" width="${size}" height="${size}" viewBox="-10 -10 20 20" style="display:block;overflow:visible" aria-hidden="true">`;
  if (gold) {
    const g = puffyStar(9.4, 0.58);
    return `${head}<path d="${handFill(g, { amp: 0.25, seed })}" fill="${c('yuzu')}" transform="translate(.6 .8)"></path><path d="${handStroke(g, { amp: 0.3, seed: seed + 1, sp: 0.6 })}" fill="none" stroke="${stroke ?? c('art-ink')}" stroke-width="1.5" stroke-linejoin="round"></path><path d="M-2.6 -4.2 L-0.7 -6.6" stroke="#FFFFFF" stroke-width="1.5" stroke-linecap="round"></path></svg>`;
  }
  return `${head}<path d="${handFill(puffyStar(9.4, 0.6), { amp: 0.25, seed })}" fill="${fill ?? 'currentColor'}" stroke="rgba(0,0,0,.22)" stroke-width=".9" stroke-linejoin="round"></path><path d="M-2.5 -4 L-0.6 -6.4" stroke="rgba(255,255,255,.75)" stroke-width="1.4" stroke-linecap="round"></path></svg>`;
}

// 开关：54 × 30 的手画轨道，荧光笔涂满表示开；圆贴纸的钮（开的时候往右移 24，由样式决定）
function switchSvg({ seed = 1, c = roleVar } = {}) {
  const fill = handStroke(P('M13 15 L41 15'), { amp: 0.6, seed, sp: 1 });
  return `<svg width="54" height="30" style="position:absolute;left:0;top:0;overflow:visible" aria-hidden="true"><path class="pm-sw-fill" pathLength="1" d="${fill}" stroke="${c('marker')}" stroke-width="20" stroke-linecap="round" fill="none"></path></svg>${wob(54, 30, { r: 15, amp: 0.5, sw: 1.6, seed: seed + 5, pad: 4, color: c('pen') })}<span class="pm-sw-knob" style="position:absolute;left:3px;top:3px;width:24px;height:24px;display:block"><svg width="24" height="24" style="display:block;overflow:visible" aria-hidden="true"><path d="${handFill(circle(12, 12, 11), { amp: 0.3, seed: seed + 10 })}" fill="${c('art-white')}" style="filter:drop-shadow(0 1px 1px ${c('shadow')})"></path><path d="${handStroke(circle(12, 12, 9.5), { amp: 0.4, seed: seed + 15, over: 0.05 })}" fill="none" stroke="${c('pen')}" stroke-width="1.6"></path></svg></span>`;
}

// 界面小图标：24 格一笔单线，跟着文字颜色（currentColor），没有填色和白边。
// d 只认绝对的 M L C Q Z；圆写成 o：[[cx, cy, r], …]；圆角方块写成 rect：[[x, y, w, h, r], …]；实心小点写成 dots：[[cx, cy, r], …]。
function glyphSvg({ d = '', o = [], rect = [], dots = [], seed = 1, size = 20, ic } = {}) {
  const sw = { amp: ic.amp, wave: ic.wave, sp: ic.sp, keep: ic.keep };
  let path = d ? handStroke(P(d), { ...sw, seed }) : '';
  o.forEach(([cx, cy, r], i) => { path += handStroke(circle(cx, cy, r), { ...sw, seed: seed + 1 + i, over: ic.over }); });
  rect.forEach(([x, y, w, h, r], i) => { path += handStroke(rrect(x, y, w, h, r), { ...sw, seed: seed + 11 + i, over: ic.over }); });
  const dot = dots.map(([cx, cy, r], i) => handFill(circle(cx, cy, r), { amp: 0.1, seed: seed + 21 + i })).join('');
  return `<svg class="pm-glyph" width="${size}" height="${size}" viewBox="0 0 24 24" style="display:block;overflow:visible" aria-hidden="true"><path d="${path}" fill="none" stroke="currentColor" stroke-width="${ic.line}" stroke-linecap="round" stroke-linejoin="round"></path>${dot ? `<path d="${dot}" fill="currentColor"></path>` : ''}</svg>`;
}

// 贴纸：sticker.mjs 的画法包一层 SVG。art 是 { cat, parts: [...] }（见 sticker.mjs）；st 是 tokens/handmade.json 的 sticker。
function stickerSvg(art, st, col, { size = 64, idp = 'pm', over = {} } = {}) {
  const { defs, html } = render(art, st, col, { k: 1, idp, over });
  return `<svg class="pm-sticker" width="${size}" height="${size}" viewBox="-8 -8 80 80" style="display:block;overflow:visible" aria-hidden="true">${defs ? `<defs>${defs}</defs>` : ''}${html}</svg>`;
}

// 贴纸的画法样张用的图形：一颗纸星星（只为看画法，不是方案的资产）
const STICKER_STAR = { cat: 'orange', parts: [{ t: 'shape', d: P('M32 8 L39 24 L56 26 L43 38 L47 55 L32 46 L17 55 L21 38 L8 26 L25 24 Z'), fill: 'cat', seed: 11 }] };

// ---------- 在页面上画 ----------

const num = (v, d) => (v === undefined || v === null || v === '' ? d : Number(v));
const LAYER = 'position:absolute;inset:0;pointer-events:none';

// 纸纹：一层分形噪声（数值来自 tokens/handmade.json 的 texture），浓淡由样式按明暗选 --pm-tex-light / --pm-tex-dark。
// 纸纹不是抖动：线条的抖动一律算进路径。
function textureSvg(t) {
  return `<svg style="position:absolute;inset:0;width:100%;height:100%;border-radius:inherit;--pm-tex-light:${t.opacity.light};--pm-tex-dark:${t.opacity.dark}" aria-hidden="true"><filter id="pm-tex" x="0" y="0" width="100%" height="100%"><feTurbulence type="fractalNoise" baseFrequency="${t.baseFrequency}" numOctaves="${t.numOctaves}" seed="3" result="n"></feTurbulence><feColorMatrix in="n" type="matrix" values="0 0 0 0 .32  0 0 0 0 .36  0 0 0 0 .26  1.6 0 0 0 -.62"></feColorMatrix></filter><rect class="pm-tex" width="100%" height="100%" filter="url(#pm-tex)"></rect></svg>`;
}

/**
 * 画 root 里（含 root 自己）所有带 data-hand 的元素。opts：
 *   params  tokens/handmade.json 的内容（必填）
 *   presets 框的预设：{ 名字: { r, amp, sw, pad, fill, stroke, tapes, stitches, tex } }。data-hand 写预设名就按它画框，
 *           元素上的 data-* 还能逐项改；r 写 "half" 是短边的一半（圆的按钮）
 *   color   角色 → CSS 值，默认 var(--color-<角色>)
 *   roles   认得的角色名；data-fill、data-stroke 写角色名时换成 color(角色)，别的值（#fff、var(--x)）原样用
 *   force   宽高没变也重画
 * 返回画了几个。
 */
function paint(root, opts = {}) {
  const ctx = makeCtx(opts);
  const hosts = [];
  if (root.matches && root.matches('[data-hand]')) hosts.push(root);
  hosts.push(...root.querySelectorAll('[data-hand]'));
  // 先定好固定大小的形状（勾选框、星星、图标……）的宽高，套着它们的元素（荧光笔）量到的才是对的
  hosts.forEach(presize);
  let n = 0;
  hosts.forEach((el, i) => { if (paintOne(el, i + 1, ctx)) n++; });
  return n;
}

const FIXED = { check: 22, star: 30, pstar: 20, icon: 20, sticker: 64 };
function presize(el) {
  const kind = el.dataset.hand;
  if (kind === 'switch') { el.style.width = '54px'; el.style.height = '30px'; return; }
  if (FIXED[kind] === undefined) return;
  const size = num(el.dataset.size, FIXED[kind]);
  el.style.width = `${size}px`;
  el.style.height = `${size}px`;
}

function makeCtx({ params, presets = {}, color = roleVar, roles = null, force = false } = {}) {
  if (!params) throw new Error('paint：缺少 params（tokens/handmade.json）');
  const known = roles ? new Set(roles) : null;
  const val = (v, fallback) => {
    if (v === undefined || v === null || v === '') return fallback === null ? null : color(fallback);
    return known && known.has(v) ? color(v) : v;
  };
  return { params, presets, c: color, val, force };
}

// 一个元素的签名：宽高加上所有 data-* 选项；没变就不重画
function signature(el, w, h) {
  const keys = Object.keys(el.dataset).filter((k) => k !== 'handSig').sort();
  return `${w}x${h}|${keys.map((k) => `${k}=${el.dataset[k]}`).join('|')}`;
}

function ensureHost(el) {
  if (typeof getComputedStyle === 'function' && getComputedStyle(el).position === 'static') el.style.position = 'relative';
  el.style.isolation = 'isolate';
}

function paintOne(el, idx, ctx) {
  const kind = el.dataset.hand;
  const d = el.dataset;
  const seed = num(d.seed, idx);
  const { params: pm, c, val } = ctx;
  const fixed = FIXED[kind] === undefined ? undefined : num(d.size, FIXED[kind]);
  presize(el);
  // 行内元素（荧光笔、划掉）按行内盒子量，只适合一行字
  const w = Math.round(el.offsetWidth), h = Math.round(el.offsetHeight);
  const sig = signature(el, w, h);
  if (!ctx.force && el.dataset.handSig === sig) return false;
  for (const old of [...el.children].filter((x) => x.hasAttribute('data-hand-painted'))) old.remove();
  ensureHost(el);

  let inner = '';
  let layerStyle = LAYER;
  let z = '';
  const fr = pm.frame;
  const preset = kind === 'frame' ? {} : ctx.presets[kind];
  if (preset) {
    // 手画的圆角框：纸、卡片、按钮、输入框。填色（fill）是手剪的边；底色、投影写在样式里时只画边。
    // 这一层整个放在内容下面（z-index -1），歪（tilt）的只是这一层：字永远是正的。
    const o = (k) => (d[k] !== undefined ? d[k] : preset[k]);
    const r = o('r') === 'half' ? Math.min(w, h) / 2 : num(o('r'), 14);
    const stroke = o('stroke');
    const tilt = num(o('tilt'), 0);
    if (o('tex') !== undefined && o('tex') !== false && o('tex') !== 'false') inner += textureSvg(pm.texture);
    inner += wob(w, h, {
      r, amp: num(o('amp'), fr.amp), sw: num(o('sw'), fr.lineWidth[1]), seed,
      fill: o('fill') ? val(o('fill'), null) : null, color: val(stroke === 'none' ? '' : stroke, 'pen'), pad: num(o('pad'), 8), stroke: stroke !== 'none',
      open: o('open') || null,
    });
    if (o('stitches') !== undefined && o('stitches') !== false) inner += stitches(h, { gap: pm.stitch.gap, len: pm.stitch.len, seed: seed + 1, color: val(o('stitches') === true ? '' : o('stitches'), 'pen'), opacity: pm.stitch.opacity });
    if (o('tapes')) {
      const colors = pm.tape.kinds.map((pair) => pair.map((role) => c(role)));
      const n = o('tapes') === 'rand' ? 'rand' : Math.min(pm.tape.maxPerCard, num(o('tapes'), 1));
      inner += tapes(w, h, seed, n, colors);
    }
    z = 'z-index:-1;';
    // 聚焦时手画的边描粗到多少（输入框），交给样式用
    layerStyle += `;--pm-sw-focus:${fr.focusLineWidth}px`;
    if (tilt) layerStyle += `;transform:rotate(${tilt}deg)`;
  } else if (kind === 'rule') {
    // 手画的横线（分割），铺满元素的宽，画在中间
    inner = hline(w, { seed, sw: num(d.sw, pm.rule.lineWidth), amp: num(d.amp, pm.rule.amp), color: val(d.stroke, 'pen'), y: Math.max(1, Math.round(h / 2)) });
  } else if (kind === 'underline') {
    inner = hline(w + 4, { seed, sw: num(d.sw, pm.rule.lineWidth), amp: num(d.amp, pm.rule.amp), color: val(d.stroke, 'pen'), style: 'position:absolute;left:-2px;bottom:-6px' });
    layerStyle = 'position:absolute;left:0;right:0;bottom:0;height:0;pointer-events:none;overflow:visible';
  } else if (kind === 'marker') {
    // 荧光笔划过：在字下面
    inner = marker(w + 8, { seed, sw: num(d.sw, pm.marker.lineWidth), opacity: num(d.opacity, pm.marker.opacity), color: val(d.stroke, 'marker') });
    z = 'z-index:-1;';
  } else if (kind === 'strike') {
    inner = strike(w, { seed, sw: num(d.sw, pm.strike.lineWidth), color: val(d.stroke, 'ink-soft') });
  } else if (kind === 'stitches') {
    inner = stitches(h, { gap: pm.stitch.gap, len: pm.stitch.len, seed, color: val(d.stroke, 'pen'), opacity: pm.stitch.opacity });
  } else if (kind === 'check') {
    inner = checkboxSvg({ seed, size: fixed, c });
  } else if (kind === 'star') {
    inner = ratingStarSvg({ seed, size: fixed, c });
  } else if (kind === 'pstar') {
    inner = paperStarSvg({ seed, size: fixed, gold: d.gold !== undefined, fill: d.fill ? val(d.fill, null) : null, stroke: d.stroke ? val(d.stroke, null) : null, c });
  } else if (kind === 'switch') {
    inner = switchSvg({ seed, c });
  } else if (kind === 'icon') {
    const list = (s, n) => (s ? s.split(',').map((x) => x.trim().split(/\s+/).map(Number)).filter((a) => a.length === n) : []);
    inner = glyphSvg({ d: d.d || '', o: list(d.o, 3), rect: list(d.rect, 5), dots: list(d.dots, 3), seed, size: fixed, ic: pm.uiIcon });
  } else if (kind === 'sticker') {
    const st = pm.sticker;
    const stage = d.stage || 'full';
    const over = { line: { outline: true, off: [0, 0], border: 0, shadow: false }, fill: { border: 0, shadow: false }, white: { shadow: false }, full: {} }[stage] ?? {};
    const art = { ...STICKER_STAR, cat: d.cat || STICKER_STAR.cat };
    const col = { ink: c('art-ink'), white: c('art-white'), shadow: c('art-shadow'), blush: c('blush'), cats: { mint: c('cat-mint'), sky: c('cat-sky'), pink: c('cat-pink'), orange: c('cat-orange'), grape: c('cat-grape') } };
    const html = stickerSvg(art, st, col, { size: fixed, idp: `pm${seed}x`, over });
    inner = stage === 'line' ? html.replace(/<path class="fl"[^>]*><\/path>/g, '') : html;
    const tilt = num(d.tilt, 0);
    if (tilt) layerStyle += `;transform:rotate(${tilt}deg)`;
  } else {
    return false;
  }
  el.insertAdjacentHTML('afterbegin', `<span data-hand-painted="${kind}" aria-hidden="true" style="${z}${layerStyle}">${inner}</span>`);
  el.dataset.handSig = signature(el, w, h);
  return true;
}

/**
 * 画一次，再在字体加载完、元素宽高变了的时候补画（只在浏览器里用）。返回停止观察的函数。
 */
function paintLive(root, opts = {}) {
  const run = () => paint(root, opts);
  run();
  if (typeof document !== 'undefined' && document.fonts && document.fonts.ready) document.fonts.ready.then(run);
  if (typeof ResizeObserver === 'undefined') return () => {};
  let queued = false;
  const ro = new ResizeObserver(() => {
    if (queued) return;
    queued = true;
    requestAnimationFrame(() => { queued = false; run(); });
  });
  const hosts = [];
  if (root.matches && root.matches('[data-hand]')) hosts.push(root);
  hosts.push(...root.querySelectorAll('[data-hand]'));
  hosts.forEach((el) => ro.observe(el));
  return () => ro.disconnect();
}

const CONFIG = {"params":{"sticker":{"grid":64,"outline":true,"line":3.3,"amp":0.75,"fillAmp":0.4,"off":[1.4,1.8],"border":4.2,"shadow":true,"tiltDeg":8},"uiIcon":{"grid":24,"line":2,"amp":0.32,"wave":7,"sp":0.6,"keep":2,"over":0.05},"frame":{"amp":0.9,"wave":26,"sp":3,"over":0.03,"drift":1.2,"lineWidth":[1.6,1.9],"focusLineWidth":2.8},"rule":{"amp":0.8,"wave":22,"lineWidth":1.6},"marker":{"amp":1.2,"wave":20,"lineWidth":12,"opacity":0.9},"strike":{"amp":0.8,"wave":14,"lineWidth":2},"stitch":{"gap":20,"len":9,"amp":0.35,"wave":6,"lineWidth":2,"opacity":0.55},"tape":{"kinds":[["tape-mint-a","tape-mint-b"],["tape-yuzu-a","tape-yuzu-b"]],"stripe":7,"opacity":0.88,"teeth":5,"toothDepth":5,"firstRotDeg":-4,"firstJitterDeg":3,"secondRotDeg":30,"maxPerCard":2},"tilt":{"cardDeg":[-2.2,2],"stickerDeg":[-8,8]},"texture":{"baseFrequency":0.85,"numOctaves":3,"opacity":{"light":0.38,"dark":0.22}}},"presets":{"frame":{},"card":{"r":14,"amp":0.95,"sw":1.9,"tex":true},"sheet":{"r":10,"amp":1.1,"sw":1.9,"tex":true,"stitches":true},"note":{"r":4,"stroke":"none"},"desk":{"r":0,"stroke":"none","tex":true},"button":{"r":9,"amp":0.55,"sw":1.6,"pad":4,"fill":"primary"},"field":{"r":11,"amp":0.6,"sw":1.6,"pad":4,"fill":"card"},"round":{"r":"half","amp":0.6,"sw":1.5,"pad":4,"fill":"card"},"cell":{"r":12,"amp":0.8,"sw":1.6},"pick":{"r":9,"amp":0.8,"pad":0,"stroke":"none","fill":"selected"},"ticket":{"r":6,"stroke":"none"}},"vars":{"desk":"--color-desk","desk-deep":"--color-desk-deep","paper":"--color-paper","grid":"--color-grid","card":"--color-card","line":"--color-line","ink":"--color-ink","ink-soft":"--color-ink-soft","pen":"--color-pen","primary":"--color-primary","on-primary":"--color-on-primary","selected":"--color-selected","marker":"--color-marker","link":"--color-link","danger":"--color-danger","on-danger":"--color-on-danger","up":"--color-up","top":"--color-top","on-top":"--color-on-top","tab":"--color-tab","on-tab":"--color-on-tab","art-ink":"--color-art-ink","art-white":"--color-art-white","art-shadow":"--color-art-shadow","shadow":"--color-shadow","cat-mint":"--color-cat-mint","cat-sky":"--color-cat-sky","cat-pink":"--color-cat-pink","cat-orange":"--color-cat-orange","cat-grape":"--color-cat-grape","yuzu":"--color-yuzu","leaf":"--color-leaf","blush":"--color-blush","heart":"--color-heart","wood":"--color-wood","tape-mint-a":"--color-tape-mint-a","tape-mint-b":"--color-tape-mint-b","tape-yuzu-a":"--color-tape-yuzu-a","tape-yuzu-b":"--color-tape-yuzu-b","sticky":"--color-sticky","on-sticky":"--color-on-sticky","print-paper":"--color-print-paper","print-cream":"--color-print-cream","print-ink":"--color-print-ink","print-ink-soft":"--color-print-ink-soft","print-rule":"--color-print-rule","print-line":"--color-print-line","kraft":"--color-kraft","kraft-inside":"--color-kraft-inside","kraft-flap":"--color-kraft-flap","thread-1":"--color-thread-1","thread-2":"--color-thread-2","thread-3":"--color-thread-3","thread-4":"--color-thread-4","cat-mint-bg":"--color-cat-mint-bg","cat-mint-tint":"--color-cat-mint-tint","cat-mint-sticker":"--color-cat-mint-sticker","cat-sky-bg":"--color-cat-sky-bg","cat-sky-tint":"--color-cat-sky-tint","cat-sky-sticker":"--color-cat-sky-sticker","cat-pink-bg":"--color-cat-pink-bg","cat-pink-tint":"--color-cat-pink-tint","cat-pink-sticker":"--color-cat-pink-sticker","cat-orange-bg":"--color-cat-orange-bg","cat-orange-tint":"--color-cat-orange-tint","cat-orange-sticker":"--color-cat-orange-sticker","cat-grape-bg":"--color-cat-grape-bg","cat-grape-tint":"--color-cat-grape-tint","cat-grape-sticker":"--color-cat-grape-sticker","yuzu-bg":"--color-yuzu-bg"}};
const NS = (typeof window !== 'undefined' ? window : globalThis).Pomelo = (typeof window !== 'undefined' ? window : globalThis).Pomelo || {};
NS.hand = { f, rng, ellipse, circle, rrect, mapCmds, rotate, move, scale, P, cmdsToD, sample, handStroke, handFill, mix, wob, hline, vline, marker, strike, tape, tapes, stitches, tapeSvg, render, svg, checkboxSvg, ratingStarSvg, paperStarSvg, switchSvg, glyphSvg, stickerSvg, STICKER_STAR, textureSvg, paint, paintLive };
NS.params = CONFIG.params;
NS.color = (role) => 'var(' + (CONFIG.vars[role] || '--color-' + role) + ')';
NS.roles = Object.keys(CONFIG.vars);
NS.presets = CONFIG.presets || {};
const withDefaults = (o) => Object.assign({ params: CONFIG.params, presets: NS.presets, color: NS.color, roles: NS.roles }, o || {});
NS.paint = (root, o) => paint(root || document, withDefaults(o));
NS.paintLive = (root, o) => paintLive(root || document, withDefaults(o));
})();
