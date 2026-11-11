// 界面检查（docs/testing.md「界面检查」）：无头 Chrome 把 ?mock 的每一页和几个编辑面板走一遍，找
//   board        盖在纸上的整块实心底（白板）          plain        浏览器画的直线框（不是手画的）
//   frame-*      手画的边画在会滚、会裁的盒子里，或被外面裁掉   covers-frame 里面涂了底色的东西压在手画的边上
//   text-out / child-out  字、格子伸出框外                BLANK        整页白了
// 用 visual-atelier 的 playwright-core 和装好的 Chrome（那边 npm ci 过）；`pnpm dev` 开着时：
//   node scripts/ui-lint.mjs [地址=http://localhost:1420] [结果.json] [窗口=1440x900,1180x780] [配色=yellow] [明暗=light]
// ROUTES=/finance,/todo 只查这几页；SHOTS=<文件夹> 存编辑面板的截图。跑的时候别改仓库里的文件（页面会刷新）。
import { createRequire } from 'node:module';
import { homedir, tmpdir } from 'node:os';
import { writeFileSync } from 'node:fs';
const require = createRequire(`${process.env.VA_DIR ?? `${homedir()}/Documents/GitHub/visual-atelier`}/package.json`);
const { chromium } = require('playwright-core');

const base = process.argv[2] || 'http://localhost:1420';
const outFile = process.argv[3] || `${tmpdir()}/pomelo-ui-lint.json`;
const sizes = (process.argv[4] || '1440x900,1180x780').split(',').map((s) => s.split('x').map(Number));
const palette = process.argv[5] || 'yellow';
const theme = process.argv[6] || 'light';
const only = process.env.ROUTES ? process.env.ROUTES.split(',') : null;

const ROUTES = ['/', '/todo', '/books', '/films', '/drawing', '/packing', '/finance', '/finance/assets', '/finance/reports', '/finance/import', '/jobs', '/jobs/table', '/jobs/stats', '/notes', '/days', '/site', '/site/publish', '/site/works', '/site/now', '/site/about', '/site/career', '/site/assets', '/site/site', '/canvas', '/atlas', '/vault', '/bookmarks', '/youzi', '/translate', '/translate/prompt', '/pub2md', '/pub2md/work', '/pub2md/prompt', '/pub2md/settings', '/seeker', '/seeker/resume', '/seeker/library', '/seeker/dossiers', '/seeker/rules', '/seeker/apply', '/seeker/history', '/seeker/settings', '/rater', '/rater/bank', '/rater/cards', '/rater/interview', '/rater/rules', '/rater/settings', '/janitor', '/janitor/disk', '/janitor/clean', '/janitor/env', '/janitor/agent', '/janitor/settings', '/calendar', '/settings', '/settings/health'];

// extra states: [route, label, async (page) => void]
const EXTRA = [
  ['/finance', 'entry panel', async (p) => p.locator('.pm-sheet tbody tr').first().click({ timeout: 2000 })],
  ['/finance', 'new entry', async (p) => p.getByRole('button', { name: /记一笔|新记录|添加/ }).first().click({ timeout: 2000 })],
  ['/finance/assets', 'account panel', async (p) => p.locator('.pm-sheet tbody tr').first().click({ timeout: 2000 })],
  ['/finance/import', 'imported', async (p) => { await p.getByRole('button', { name: '导入', exact: true }).click({ timeout: 2000 }); await p.waitForTimeout(2500); }],
  ['/finance/import', 'imported row', async (p) => { await p.getByRole('button', { name: '导入', exact: true }).click({ timeout: 2000 }); await p.waitForTimeout(2500); await p.locator('.pm-sheet tbody tr').first().click({ timeout: 2000 }); }],
  ['/todo', 'task panel', async (p) => p.locator('.pm-sheet .task-title').first().click({ timeout: 2000 })],
  ['/books', 'book panel', async (p) => p.locator('.pm-sheet [data-auto-card] button, .pm-sheet tbody tr').first().click({ timeout: 2000 })],
  ['/jobs/table', 'job panel', async (p) => p.locator('.pm-sheet tbody tr').first().click({ timeout: 2000 })],
  ['/days', 'day panel', async (p) => p.locator('.pm-sheet [data-auto-card] button').first().click({ timeout: 2000 })],
  ['/seeker/resume', 'drawer assistant', async (p) => p.getByRole('button', { name: /岗位与建议|建议/ }).first().click({ timeout: 2000 })],
  ['/seeker/resume', 'drawer materials', async (p) => p.getByRole('button', { name: /选材/ }).first().click({ timeout: 2000 })],
];

const lint = () => {
  const out = [];
  const sheet = document.querySelector('.pm-page.front main.pm-sheet') || document.querySelector('.deskpage') || document.body;
  const root = document.querySelector('#root');
  const vis = (el) => { const cs = getComputedStyle(el); if (cs.display === 'none' || cs.visibility === 'hidden' || Number(cs.opacity) === 0) return false; const r = el.getBoundingClientRect(); return r.width > 0 && r.height > 0; };
  const name = (el) => `${el.tagName.toLowerCase()}${el.id ? '#' + el.id : ''}.${String(el.className).replace(/\s+/g, '.').slice(0, 90)}${el.getAttribute('aria-label') ? `[${el.getAttribute('aria-label')}]` : ''}`;
  const alpha = (c) => { if (!c || c === 'transparent') return 0; const m = c.match(/\/\s*([\d.]+)\)/) || c.match(/rgba\([^,]+,[^,]+,[^,]+,\s*([\d.]+)\)/); return m ? Number(m[1]) : 1; };
  const clips = (cs) => cs.overflowX !== 'visible' || cs.overflowY !== 'visible';
  const sr = sheet.getBoundingClientRect();
  // 1. white boards: big opaque fills on the paper that aren't a painted card
  for (const el of sheet.querySelectorAll('*')) {
    if (el.closest('[data-hand-painted]') || !vis(el)) continue;
    if (el.closest('[data-hand]:not(.pm-sheet), [data-auto-card], .pm-panel, [role=dialog], dialog, [popover]') ) continue;
    const cs = getComputedStyle(el);
    if (alpha(cs.backgroundColor) < 0.85) continue;
    const r = el.getBoundingClientRect();
    if (r.width * r.height < 0.12 * sr.width * sr.height && !(r.width > 220 && r.height > 320)) continue;
    out.push({ kind: 'board', el: name(el), w: Math.round(r.width), h: Math.round(r.height), bg: cs.backgroundColor });
  }
  // 2. plain CSS boxes: drawn by the browser (straight lines), not by hand
  for (const el of root.querySelectorAll('*')) {
    if (el.closest('[data-hand-painted],[data-nolint]') || el.dataset.hand || el.hasAttribute('data-auto-card') || !vis(el)) continue;
    // small chips (附件、文件名的小签) keep a fine line on purpose
    if (el.tagName === 'BUTTON' && el.getBoundingClientRect().height <= 34 && el.getBoundingClientRect().width <= 240) continue;
    if (/^(INPUT|IMG|IFRAME|VIDEO|CANVAS|TEXTAREA|SELECT|svg|path)$/i.test(el.tagName)) continue;
    const cs = getComputedStyle(el);
    const sides = ['Top', 'Right', 'Bottom', 'Left'].filter((s) => cs[`border${s}Style`] === 'solid' && parseFloat(cs[`border${s}Width`]) >= 1 && alpha(cs[`border${s}Color`]) > 0.25).length;
    if (sides < 3) continue;
    const r = el.getBoundingClientRect();
    if (r.width < 90 || r.height < 28) continue;
    out.push({ kind: 'plain', el: name(el), w: Math.round(r.width), h: Math.round(r.height) });
  }
  // 3. hand frames cut off: the frame lives inside its own scroller / clip, or an ancestor clips its edge
  for (const p of root.querySelectorAll('[data-hand-painted]')) {
    const host = p.parentElement;
    if (!host || !vis(host) || host.matches('.pm-sheet, body')) continue;
    if (['strike', 'pick', 'underline', 'line', 'stitch', 'mark'].includes(host.dataset.hand)) continue;
    const hcs = getComputedStyle(host);
    const hr = host.getBoundingClientRect();
    if (clips(hcs) && (host.scrollHeight > host.clientHeight + 2 || host.scrollWidth > host.clientWidth + 2)) out.push({ kind: 'frame-scrolls', el: name(host), hand: host.dataset.hand });
    else if (clips(hcs) && hcs.overflowX !== 'clip') out.push({ kind: 'frame-self-clip', el: name(host), hand: host.dataset.hand, ov: `${hcs.overflowX}/${hcs.overflowY}` });
    for (let a = host.parentElement; a && a !== document.body; a = a.parentElement) {
      const cs = getComputedStyle(a);
      if (!clips(cs)) continue;
      const ar = a.getBoundingClientRect();
      const cl = { l: ar.left + a.clientLeft, t: ar.top + a.clientTop, r: ar.left + a.clientLeft + a.clientWidth, b: ar.top + a.clientTop + a.clientHeight };
      const ex = { l: cl.l - hr.left, t: cl.t - hr.top, r: hr.right - cl.r, b: hr.bottom - cl.b };
      const more = { t: a.scrollTop > 0, b: a.scrollTop + a.clientHeight < a.scrollHeight - 1, l: a.scrollLeft > 0, r: a.scrollLeft + a.clientWidth < a.scrollWidth - 1 };
      const cut = Object.entries(ex).filter(([k, v]) => v > -1.5 && v <= 10 && !more[k]).map(([k, v]) => `${k}${v.toFixed(1)}`);
      // partly scrolled out of a page that scrolls is fine; a frame that touches / pokes the clip edge isn't
      if (cut.length && !Object.values(ex).some((v) => v > 10)) out.push({ kind: 'frame-cut', el: name(host), by: name(a), cut: cut.join(' ') });
      break;
    }
  }
  // 4. text past its frame
  const boxes = [...root.querySelectorAll('[data-hand]:not(.pm-sheet):not([data-hand=strike]):not([data-hand=pick]), .pm-seg, button')].filter(vis);
  for (const el of boxes) {
    const cs = getComputedStyle(el);
    if (cs.overflowX !== 'visible' && el.scrollWidth > el.clientWidth + 1 && !/(auto|scroll)/.test(cs.overflowX)) { out.push({ kind: 'text-clipped', el: name(el), sw: el.scrollWidth, cw: el.clientWidth, text: el.textContent.trim().slice(0, 40) }); continue; }
    const r = el.getBoundingClientRect();
    const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
    let bad = null;
    for (let n = walker.nextNode(); n && !bad; n = walker.nextNode()) {
      if (!n.textContent.trim()) continue;
      const pe = n.parentElement;
      if (!pe || pe.closest('[data-hand-painted],[data-nolint],.sr-only,[aria-hidden=true]')) continue;
      // a nested scroller / clip owns its own text
      let inner = false;
      for (let a = pe; a && a !== el; a = a.parentElement) { const c = getComputedStyle(a); if (clips(c) || c.position === 'absolute' || c.position === 'fixed') { inner = true; break; } }
      if (inner || !vis(pe)) continue;
      const range = document.createRange();
      range.selectNodeContents(n);
      for (const t of range.getClientRects()) {
        if (t.width < 1) continue;
        const d = Math.max(t.right - r.right, r.left - t.left, t.bottom - r.bottom - 2, r.top - t.top - 2);
        if (d > 1.5) { bad = { text: n.textContent.trim().slice(0, 30), d: Math.round(d) }; break; }
      }
    }
    if (bad) out.push({ kind: 'text-out', el: name(el), ...bad });
  }
  // 6. a child box past its hand frame (a segmented item, a field's contents)
  for (const host of [...root.querySelectorAll('[data-hand]')].filter(vis)) {
    if (['sheet', 'strike', 'pick', 'underline', 'line', 'stitch', 'mark'].includes(host.dataset.hand)) continue;
    const hcs = getComputedStyle(host);
    if (clips(hcs)) continue;
    const r = host.getBoundingClientRect();
    let bad = null;
    for (const c of host.querySelectorAll('*')) {
      if (c.closest('[data-hand-painted],[data-nolint],[popover],dialog') || !vis(c)) continue;
      let inner = false;
      for (let a = c; a && a !== host; a = a.parentElement) { const cc = getComputedStyle(a); if (cc.position === 'absolute' || cc.position === 'fixed' || (a !== c && clips(cc))) { inner = true; break; } }
      if (inner) continue;
      const cr = c.getBoundingClientRect();
      if (cr.width < 2 || cr.height < 2) continue;
      const d = Math.max(cr.right - r.right, r.left - cr.left, cr.bottom - r.bottom, r.top - cr.top);
      if (d > 2) { bad = { child: name(c).slice(0, 60), text: c.textContent.trim().slice(0, 24), d: Math.round(d) }; break; }
    }
    if (bad) out.push({ kind: 'child-out', el: name(host), ...bad });
  }
  // 7. an opaque box inside a hand card / field lying over its edge (the frame is drawn under the contents)
  for (const host of [...root.querySelectorAll('[data-hand=card], [data-hand=field], [data-auto-card]')].filter(vis)) {
    const r = host.getBoundingClientRect();
    for (const c of host.querySelectorAll('*')) {
      if (c.closest('[data-hand-painted],[popover],dialog,[role=menu],[role=listbox]') || c.dataset.hand || !vis(c)) continue;
      if (c.closest('[data-hand]') !== host && c.closest('[data-hand]') && host.contains(c.closest('[data-hand]')) && c.closest('[data-hand]') !== host) continue;
      const cs = getComputedStyle(c);
      if (alpha(cs.backgroundColor) < 0.85) continue;
      const cr0 = c.getBoundingClientRect();
      const ins = (cs.clipPath.match(/inset\(([^)]*)\)/)?.[1] ?? '0').split(/\s+/).map(parseFloat);
      const [it, ir = it, ib = it, il = ir] = ins;
      const cr = { left: cr0.left + il, right: cr0.right - ir, top: cr0.top + it, bottom: cr0.bottom - ib, width: cr0.width - il - ir, height: cr0.height - it - ib };
      const near = [cr.left - r.left < 3 && cr.height > r.height * 0.3, r.right - cr.right < 3 && cr.height > r.height * 0.3, cr.top - r.top < 3 && cr.width > r.width * 0.3, r.bottom - cr.bottom < 3 && cr.width > r.width * 0.3];
      if (near.some(Boolean)) { out.push({ kind: 'covers-frame', el: name(host), child: name(c).slice(0, 70), bg: cs.backgroundColor }); break; }
    }
  }
  // 5. render errors shown by the page boundary
  if (!root.children.length || (root.innerText || '').trim().length < 5) out.push({ kind: 'BLANK' });
  return out;
};

const browser = await chromium.launch({ channel: 'chrome', args: ['--hide-scrollbars'] });
const report = [];
for (const [w, h] of sizes) {
  const ctx = await browser.newContext({ viewport: { width: w, height: h }, deviceScaleFactor: 1 });
  const page = await ctx.newPage();
  const errs = [];
  page.on('pageerror', (e) => errs.push(String(e.message)));
  page.on('console', (m) => { if (m.type() === 'error') errs.push(m.text().slice(0, 300)); });
  await page.addInitScript(([pal, th]) => { localStorage.setItem('pomelo.appearance', JSON.stringify({ palette: pal, theme: th })); }, [palette, theme]);
  await page.goto(`${base}/?mock#/`);
  await page.waitForTimeout(1500);
  await page.evaluate(([pal, th]) => { document.documentElement.dataset.palette = pal; document.documentElement.dataset.theme = th; }, [palette, theme]);
  const settle = async () => { await page.waitForTimeout(900); await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(() => setTimeout(r, 120))))); };
  const go = async (route) => { await page.evaluate((r) => { location.hash = '#' + r; }, route); await settle(); await page.evaluate(([pal, th]) => { document.documentElement.dataset.palette = pal; document.documentElement.dataset.theme = th; }, [palette, theme]); await settle(); };
  for (const route of ROUTES) {
    if (only && !only.includes(route)) continue;
    errs.length = 0;
    await go(route);
    report.push({ size: `${w}x${h}`, route, state: '', items: await page.evaluate(lint), errs: [...errs] });
  }
  for (const [route, label, act] of EXTRA) {
    if (only && !only.includes(route)) continue;
    errs.length = 0;
    await page.goto(`${base}/?mock#${route}`);
    await page.waitForTimeout(1200);
    await page.evaluate(([pal, th]) => { document.documentElement.dataset.palette = pal; document.documentElement.dataset.theme = th; }, [palette, theme]);
    let ok = true;
    try { await act(page); } catch (e) { ok = false; }
    await settle();
    report.push({ size: `${w}x${h}`, route, state: label + (ok ? '' : ' (not reached)'), items: ok ? await page.evaluate(lint) : [], errs: [...errs] });
    if (process.env.SHOTS) await page.screenshot({ path: `${process.env.SHOTS}/${w}-${route.replace(/\//g, '_')}-${label.replace(/\s/g, '_')}.png` });
  }
  await ctx.close();
}
await browser.close();
writeFileSync(outFile, JSON.stringify(report, null, 1));
const flat = report.flatMap((r) => r.items.map((i) => ({ at: `${r.size} ${r.route}${r.state ? ' · ' + r.state : ''}`, ...i })));
const by = {};
for (const f of flat) (by[f.kind] ??= []).push(f);
for (const [k, v] of Object.entries(by)) {
  console.log(`\n## ${k} (${v.length})`);
  const seen = new Set();
  for (const f of v) { const key = `${f.el} ${f.cut ?? ''} ${f.text ?? ''}`; if (seen.has(key)) continue; seen.add(key); console.log(`- ${f.at} | ${f.el}${f.by ? ' ⟵ ' + f.by : ''}${f.cut ? ' ' + f.cut : ''}${f.text ? ` "${f.text}" +${f.d ?? ''}` : ''}${f.w ? ` ${f.w}x${f.h}` : ''}${f.ov ? ' ' + f.ov : ''}${f.child ? ' ⊃ ' + f.child : ''}`); }
}
const errAll = report.filter((r) => r.errs.length).map((r) => `${r.size} ${r.route} ${r.state}: ${r.errs.slice(0, 3).join(' | ')}`);
if (errAll.length) console.log('\n## errors\n' + errAll.join('\n'));
