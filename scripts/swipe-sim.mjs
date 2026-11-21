// 见字如面's two-finger swipes can't be tried without a trackpad, so this plays made-up wheel streams to swipe.ts's Glide
// the way useSwipes folds columns, and checks how many columns each folds: one swipe, one column — clean, with the page
// busy (events lumped into one), with the app's main thread busy (events late, then lumped), with event times that are
// when they got through; and two or three swipes in a row still count each. `node scripts/swipe-sim.mjs [seed] [all]`
// (Node strips the types of swipe.ts itself); exits 1 when a case is off.
// An event: [its own time, delta, awake (ms the page ran without a wheel event before it), slept (it stood still meanwhile)].
import { Glide } from '../src/modules/articles/swipe.ts';

let seed = Number(process.argv[2] ?? 7);
const rnd = () => ((seed = (seed * 1103515245 + 12345) & 0x7fffffff) / 0x7fffffff);

/** One swipe: fingers ramp up to `top` px/frame over `ramp` frames, hold, lift → momentum decays (×k per frame), cut after `cut` ms. */
function swipe(t0, { hz = 60, top = 30, ramp = 5, hold = 6, k = 0.94, dir = 1, cut = Infinity, first = 1.5 } = {}) {
  const step = 1000 / hz, s = 60 / hz, out = [];
  let t = t0;
  for (let i = 1; i <= ramp; i++, t += step) out.push([t, dir * top * s * (i / ramp) ** first]);
  for (let i = 0; i < hold; i++, t += step) out.push([t, dir * top * s]);
  const lift = t;
  for (let d = top * s * 0.95; d > 0.4 && t - lift < cut; d *= k ** s, t += step) out.push([t, dir * d]);
  return out;
}
/** As made: every event at its time, the page awake between them. */
const clean = (ev) => ev.map(([t, d], i) => [t, d, i ? t - ev[i - 1][0] : Infinity, false]);
/** The page asleep from a to b: what was made then comes as one (deltas added, the last one's time), no silence. */
const asleep = (ev, a, b) => {
  const inside = ev.filter(([t]) => t >= a && t < b);
  if (inside.length < 2) return ev;
  return [...ev.filter(([t]) => t < a), [inside.at(-1)[0], inside.reduce((n, [, d]) => n + d, 0), Math.min(16, a - (ev.filter(([t]) => t < a).at(-1)?.[0] ?? a)) + 12, true], ...ev.filter(([t]) => t >= b).map((e, i) => (i ? e : [e[0], e[1], Math.min(e[2], e[0] - b), e[3]]))];
};
/** The app (not the page) busy from a to b: the first event made then comes late with its own time — the page was awake
 * all the while — and the rest as one right behind it. */
const appBusy = (ev, a, b) => {
  const inside = ev.filter(([t]) => t >= a && t < b);
  if (inside.length < 3) return ev;
  const rest = inside.slice(1);
  return [...ev.filter(([t]) => t < a), [inside[0][0], inside[0][1], b - a, false], [rest.at(-1)[0], rest.reduce((n, [, d]) => n + d, 0), 0, false], ...ev.filter(([t]) => t >= b)];
};
/** Busy frames from a to b: each `frame` ms or so of events come as one. */
const janky = (ev, a, b, frame, how = asleep) => {
  let out = ev;
  for (let t = a; t < b; ) {
    const w = frame * (0.6 + rnd() * 0.8);
    out = how(out, t, t + w);
    t += w;
  }
  return out;
};
/** Times as when it got through, evenly paced (if an event's time were not its own). */
const repaced = (ev, a, b) => {
  const idx = ev.map(([t], i) => (t >= a && t < b ? i : -1)).filter((i) => i >= 0);
  if (idx.length < 2) return ev;
  const [t0, t1] = [ev[idx[0]][0], ev[idx.at(-1)][0]];
  return ev.map((e, i) => (idx.includes(i) ? [t0 + ((t1 - t0) * idx.indexOf(i)) / (idx.length - 1), e[1], e[2], e[3]] : e));
};

function folds(G, ev) {
  const g = new G();
  let owner = { id: -1, done: true }, n = 0, last = -1e9;
  const at = [];
  for (const [t, dx, awake, slept] of ev) {
    if (t - last > 140 && awake > 140) owner.done = true; // the idle timer
    last = t;
    if (g.feed(dx, t, awake, slept)) owner = { id: g.id, done: false };
    if (owner.done || owner.id !== g.id) continue;
    if (g.gliding) { owner.done = true; continue; }
    if (Math.abs(g.acc) >= 50) { owner.done = true; g.rest?.(t); n += Math.sign(g.acc); at.push(Math.round(t)); }
  }
  return `${n}${at.length > 1 ? ` (${at.join(', ')})` : ''}`;
}

const cases = [];
const add = (name, want, ev) => cases.push([name, want, ev]);
for (const hz of [60, 120]) {
  for (const top of [14, 30, 70]) {
    const base = clean(swipe(0, { hz, top }));
    const lift = (5 + 6) * (1000 / hz);
    const two = (cut, gap, o = {}) => clean([...swipe(0, { hz, top, cut }), ...swipe(lift + cut + gap, { hz, top, ...o })]);
    const H = `${hz}Hz 顶速${top} `;
    add(H + '干净', 1, base);
    add(H + '收栏后页面忙 300ms（每 50ms 一批）', 1, janky(base, lift, lift + 300, 50));
    add(H + '整段滑行页面都忙（每 34ms 一批）', 1, janky(base, lift, 2500, 34));
    add(H + '页面卡 250ms', 1, asleep(base, lift + 10, lift + 260));
    add(H + '页面卡 450ms', 1, asleep(base, lift + 10, lift + 460));
    add(H + '滑行中页面卡 200ms', 1, asleep(base, lift + 300, lift + 500));
    add(H + '还没收栏页面就卡 250ms', 1, asleep(base, 30, 280));
    add(H + '主线程卡 250ms（保存、刷新）', 1, appBusy(base, lift + 10, lift + 260));
    add(H + '主线程卡 450ms', 1, appBusy(base, lift + 10, lift + 460));
    add(H + '主线程一阵一阵卡（每 60ms）', 1, janky(base, lift, lift + 900, 60, appBusy));
    add(H + '还没收栏主线程就卡 250ms', 1, appBusy(base, 30, 280));
    add(H + '页面忙 + 时间按送到的算', 1, repaced(janky(base, lift, lift + 900, 40), lift, lift + 900));
    add(H + '页面忙到滑行快停 + 时间按送到的算', 1, repaced(janky(base, lift, 2500, 40), lift, 2500));
    add(H + '停 500ms 再滑一次', 2, two(250, 500));
    add(H + '滑行中接着滑（抬手 350ms 后，停 70ms）', 2, two(350, 70));
    add(H + '滑行中很快接着滑（抬手 150ms 后，停 40ms）', 2, two(150, 40));
    add(H + '滑行中接着滑，第一下就不小', 2, two(300, 50, { first: 0.6 }));
    add(H + '滑行中接着滑 + 收栏时页面忙', 2, janky(two(350, 70), lift, lift + 280, 40));
    add(H + '连滑三次', 3, clean([...swipe(0, { hz, top, cut: 300 }), ...swipe(lift + 380, { hz, top, cut: 300 }), ...swipe(2 * (lift + 380), { hz, top })]));
    add(H + '左滑后马上右滑', 0, two(200, 60, { dir: -1 }));
  }
  const slow = clean(Array.from({ length: Math.round(2 * hz) }, (_, i) => [i * (1000 / hz), 400 / (2 * hz)]));
  add(`${hz}Hz 不抬手慢慢拖 400px`, 1, slow);
  add(`${hz}Hz 不抬手慢慢拖 400px + 页面忙`, 1, janky(slow, 300, 1800, 45));
  add(`${hz}Hz 不抬手慢慢拖 400px + 主线程卡`, 1, janky(slow, 300, 1800, 70, appBusy));
}
let bad = 0;
const show = process.argv[3] === 'all';
for (const [name, want, ev] of cases) {
  const n = folds(Glide, ev);
  const ok = n.split(' ')[0] === String(want);
  if (!ok) bad++;
  if (!ok || show) console.log(`${ok ? ' ' : '✗'} 要 ${want} | 收了 ${n.padEnd(18)} | ${name}`);
}
console.log(`${cases.length} 种，${bad} 种不对`);
process.exit(bad ? 1 : 0);
