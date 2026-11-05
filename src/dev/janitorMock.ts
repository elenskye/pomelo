// DevJanitor in the browser mock (`?mock`): the fake Mac as Rust scans it (janitorMock.json, made by
// `janitor_mock_json` in youzi/assistants/janitor/tests.rs), its times moved to now. 重新扫描 runs for two
// seconds; cleaning and the Trash take items off and their sizes off the folders holding them.
import data from "./janitorMock.json";
import type { JanitorCard, JanitorChoice, JanitorEnv, JanitorGroup, JanitorItem, JanitorOutcome, JanitorOverview, JanitorPrompt, JanitorRing, JanitorRule, JanitorSettings } from "@/modules/youzi/assistants/janitorApi";

type Args = Record<string, unknown>;
type Node = { id: number; parent: number | null; name: string; path: string; kind: string; size: number; files: number; newest: number; denied: number };

const shift = Math.floor(Date.now() / 1000) - (data.overview.summary?.scannedAt ?? 0);
const moved = (t: number) => (t ? t + shift : 0);
const base = data.overview as unknown as JanitorOverview;
let overview: JanitorOverview = {
  ...base,
  summary: base.summary ? { ...base.summary, scannedAt: base.summary.scannedAt + shift } : null,
  categories: base.categories.map((g) => ({ ...g, items: g.items.map((i) => ({ ...i, newest: moved(i.newest) })) })),
  groups: base.groups.map((g) => ({ ...g, items: g.items.map((i) => ({ ...i, newest: moved(i.newest) })) })),
};
const nodes: Node[] = (data.nodes as Node[]).map((n) => ({ ...n, newest: moved(n.newest) }));
const envs: JanitorEnv[] = (data.envs as unknown as JanitorEnv[]).map((e) => ({ ...e, items: e.items.map((i) => ({ ...i, newest: moved(i.newest) })) }));
let settings: JanitorSettings = { off: [], staleDays: 30 };
let rules: JanitorRule[] = [];
const SKILL = "## 工作方式（DevJanitor 的 SKILL）\n1. 先看下面「现在的情况」；要细节用 pomelo_janitor_look（模拟）。\n2. 只读的命令用 pomelo_janitor_run 直接跑。\n3. 只能提议，用户在卡片上点了才执行。";
let skill: string | null = null;
const prompt = (): JanitorPrompt => ({ skill: skill ?? SKILL, custom: skill != null, head: "你是 DevJanitor，柚儿园里帮用户管这台 Mac 磁盘的助手（模拟）。个人文件夹是 /Users/me。", situation: "（模拟）磁盘 494.4 GB，还剩 200.1 GB。\n清理页有一份清单：一键清理 9 类 31 项，能清 22.6 GB（用户自己点）；只列出来、等用户挑的 7 组。要看清单里有什么，用 pomelo_janitor_look 的 cleanup。" });

/** The Agent's conversation (mockIpc makes it) and its model. */
export const janitorChat: { id: number | null; choice: JanitorChoice } = { id: null, choice: { model: null, thinking: true, effort: null } };
/** Its cards, by reply. */
export const janitorCards = new Map<number, JanitorCard[]>();
let nextCard = 1;

const blank = (kind: JanitorCard["kind"], over: Partial<JanitorCard>): JanitorCard => ({ id: nextCard++, runId: 0, kind, state: "pending", title: "", items: [], command: "", what: "", deletes: "", recoverable: "", frees: "", sudo: false, protected: null, rule: null, matches: 0, matchSize: 0, output: "", code: null, error: null, ...over });

/** agent.rs `mock_calls` / `mock_answer`: what the mock Agent looks at and proposes, by the question. */
export function janitorReply(prompt: string, msgId: number) {
  const q = (label: string) => ({ module: null, label, state: "ok", result: "（模拟）", records: [] });
  const queries = [q(prompt.includes("环境") ? "DevJanitor · 开发环境" : "DevJanitor · 总的情况")];
  if (prompt.includes("看看") || prompt.includes("文件夹")) queries.push(q("DevJanitor · 看 /Users/me/Downloads"));
  if (prompt.includes("只读") || prompt.includes("du")) queries.push({ ...q("DevJanitor · 跑了 du -sh ~/Library/Caches"), result: "$ du -sh ~/Library/Caches\n（假 Mac：开发版和测试不真跑命令，这是编的结果）\n12M\t/Users/me/Library/Caches/com.example.tool" });
  if (prompt.includes("清单")) queries.push(q("DevJanitor · 清理页的清单"));
  const cards: JanitorCard[] = [];
  if (prompt.includes("删") || prompt.includes("废纸篓"))
    cards.push(blank("trash", { title: "用不上的东西", items: [
      { path: "/Users/me/.mystery-tool", why: "（模拟）认不出来，90 天没动", size: 310e6, protected: null, kind: "文件夹", done: false, error: null },
      { path: "/Users/me/Downloads/旧版微信.dmg", why: "（模拟）装过了的安装包", size: 300e6, protected: null, kind: "文件", done: false, error: null },
      { path: "/Users/me/Documents/LLM/models/embedding.gguf", why: "（模拟）受保护的例子", size: 600e6, protected: "本地模型", kind: "文件", done: false, error: null },
    ], protected: "本地模型" }));
  if (prompt.includes("命令") || prompt.includes("conda")) cards.push(blank("command", { title: "清 Conda", command: "conda clean --all --yes", what: "（模拟）清掉 Conda 没在用的包和下载的压缩包", deletes: "pkgs 里没被任何环境用到的包", recoverable: "以后装包时会重新下载", frees: "约 3 GB" }));
  if (prompt.includes("sudo") || prompt.includes("JDK")) cards.push(blank("command", { title: "删 JDK 8", command: "sudo rm -rf /Library/Java/JavaVirtualMachines/jdk-1.8.jdk", what: "（模拟）删掉 JDK 8", deletes: "JDK 8 整个文件夹", recoverable: "要用时重新装", frees: "约 210 MB", sudo: true }));
  if (prompt.includes("规则") || prompt.includes("一键清理")) cards.push(blank("rule", { title: "Whisper 的模型缓存", rule: { id: 0, name: "Whisper 的模型缓存", pattern: "/Users/me/.cache/whisper", recognize: "（模拟）Whisper 下载的模型", rebuild: "用到时重新下载", checked: false, createdAt: "" }, matches: 1, matchSize: 1.5e9 }));
  cards.forEach((c) => (c.runId = msgId));
  if (cards.length) janitorCards.set(msgId, cards);
  const text = cards.length ? `这是开发版的**模拟回答**：看了 ${queries.length} 处，放进了 ${cards.length} 张卡片，看过没问题再点。` : `这是开发版的**模拟回答**：看了 ${queries.length} 处。说「删」「命令」「规则」我会提议卡片。`;
  return { queries, text };
}

const findCard = (id: number) => [...janitorCards.values()].flat().find((c) => c.id === id);
const pendingCard = (id: number) => {
  const c = findCard(id);
  if (!c) throw new Error("这张卡片已经不在了");
  if (c.state !== "pending") throw new Error("这张卡片已经处理过了");
  return c;
};
let scanUntil = 0;

const scanState = () => {
  const left = scanUntil - Date.now();
  if (left <= 0) return { ...overview.scan, running: false };
  const done = 1 - left / 2000;
  return { running: true, phase: done < 0.7 ? "扫描磁盘" : "找重复的文件", files: Math.round(done * 1_284_000), bytes: Math.round(done * 236e9), startedAt: Math.floor(Date.now() / 1000), error: null };
};

const total = (gs: JanitorGroup[]) => gs.map((g) => ({ ...g, size: g.items.reduce((s, i) => s + i.size, 0) })).filter((g) => g.items.length);

function view(): JanitorOverview {
  const s = scanState();
  if (!s.running && scanUntil) {
    scanUntil = 0;
    overview = { ...overview, summary: overview.summary && { ...overview.summary, scannedAt: Math.floor(Date.now() / 1000) } };
  }
  const categories = overview.categories.filter((g) => !settings.off.includes(g.id)).map((g) => ({ ...g, name: settings.names?.[g.id] ?? rules.find((r) => `rule:${r.id}` === g.id)?.name ?? g.name }));
  return { ...overview, scan: s, categories, cleanable: categories.flatMap((g) => g.items).filter((i) => !i.inUse).reduce((a, i) => a + i.size, 0) };
}

function shrink(path: string, by: number) {
  for (const n of nodes) {
    if (n.path === "/" || path.startsWith(`${n.path}/`)) n.size = Math.max(0, n.size - by);
  }
  for (let i = nodes.length - 1; i >= 0; i--) if (nodes[i].path === path || nodes[i].path.startsWith(`${path}/`)) nodes.splice(i, 1);
}

function ring(n: Node, depth: number): JanitorRing {
  const kids = depth > 0 ? nodes.filter((k) => k.parent === n.id && (k.size >= 20e6 || n.parent === null)).sort((a, b) => b.size - a.size).slice(0, 14) : [];
  const children = kids.map((k) => ring(k, depth - 1));
  const shown = depth > 0 ? kids.reduce((s, k) => s + k.size, 0) : n.size;
  return { path: n.path, name: n.name, kind: n.kind as JanitorRing["kind"], size: n.size, files: n.files, denied: n.denied, newest: n.newest, rest: Math.max(0, n.size - shown), children };
}

function take(ids: number[], listed: boolean, ok: (i: JanitorItem) => string | null): JanitorOutcome {
  const out: JanitorOutcome = { freed: 0, done: 0, failed: [] };
  const key = listed ? "groups" : "categories";
  const groups = overview[key].map((g) => ({
    ...g,
    items: g.items.filter((i) => {
      if (!ids.includes(i.id)) return true;
      const why = ok(i);
      if (why) {
        out.failed.push([i.title, why]);
        return true;
      }
      out.freed += i.size;
      out.done += 1;
      i.paths.forEach((p) => shrink(p, i.size / i.paths.length));
      return false;
    }),
  }));
  overview = { ...overview, [key]: total(groups) };
  return out;
}

export const janitorHandlers: Record<string, (a: Args) => unknown> = {
  janitor_overview: () => view(),
  janitor_scan: (a) => {
    if (a.force) scanUntil = Date.now() + 2000;
    return scanState();
  },
  janitor_status: () => scanState(),
  janitor_stop: () => {
    scanUntil = 0;
    return null;
  },
  janitor_ring: (a) => {
    const n = nodes.find((x) => x.path === a.path) ?? nodes.find((x) => x.path === "/");
    if (!n) return null;
    const crumbs: [string, string][] = [];
    let p = nodes.find((x) => x.id === n.parent);
    while (p) {
      crumbs.unshift([p.path, p.name]);
      const parent = p.parent;
      p = nodes.find((x) => x.id === parent);
    }
    return { node: ring(n, 3), crumbs };
  },
  janitor_clean: (a) => take(a.ids as number[], false, (i) => (i.inUse ? `在用（${i.inUse}）` : null)),
  janitor_trash: (a) => {
    const out = take(a.ids as number[], true, (i) => (i.protected && !a.protectedOk ? "受保护，要再确认一次" : null));
    if (overview.summary) overview = { ...overview, summary: { ...overview.summary, trash: { size: overview.summary.trash.size + out.freed, count: overview.summary.trash.count + out.done } } };
    return out;
  },
  janitor_empty_trash: () => {
    const t = overview.summary?.trash ?? { size: 0, count: 0 };
    if (overview.summary) overview = { ...overview, summary: { ...overview.summary, trash: { size: 0, count: 0 } } };
    shrink(`${overview.home}/.Trash`, t.size);
    return { freed: t.size, done: t.count, failed: [] };
  },
  janitor_envs: () => envs,
  janitor_memory: () => ({ total: 18e9, used: 11.4e9, pressure: "normal", top: [{ name: "Google Chrome", bytes: 3.1e9 }, { name: "Claude", bytes: 1.4e9 }, { name: "Pomelo OS", bytes: 0.4e9 }, { name: "Code", bytes: 0.9e9 }, { name: "WindowServer", bytes: 0.3e9 }] }),
  janitor_reveal: () => null,
  janitor_open_privacy: () => null,
  janitor_settings: () => ({ settings, categories: CATEGORIES, rules }),
  janitor_settings_set: (a) => {
    settings = a.settings as JanitorSettings;
    return { settings, categories: CATEGORIES, rules };
  },
  janitor_rule_rename: (a) => {
    const name = String(a.name).trim();
    if (!name) throw new Error("名字不能是空的");
    rules = rules.map((r) => (r.id === a.id ? { ...r, name } : r));
    return { settings, categories: CATEGORIES, rules };
  },
  janitor_rule_delete: (a) => {
    rules = rules.filter((r) => r.id !== a.id);
    return { settings, categories: CATEGORIES, rules };
  },
  janitor_agent_prompt: () => prompt(),
  janitor_agent_skill: (a) => {
    const t = typeof a.skill === "string" ? a.skill.trim() : "";
    skill = t && t !== SKILL ? t : null;
    return prompt();
  },
  janitor_card: (a) => {
    const c = findCard(a.id as number);
    if (!c) throw new Error("这张卡片已经不在了");
    return structuredClone(c);
  },
  janitor_card_trash: (a) => {
    const c = pendingCard(a.id as number);
    const picked = a.picked as number[];
    if (!picked.length) throw new Error("至少勾上一项");
    c.items.forEach((it, i) => {
      const ok = picked.includes(i) && (!it.protected || !!a.protectedOk);
      Object.assign(it, { done: ok, error: ok || !picked.includes(i) ? null : "受保护，要再确认一次" });
    });
    c.state = c.items.some((it) => it.done) ? "done" : "failed";
    return structuredClone(c);
  },
  janitor_card_run: (a) => {
    const c = pendingCard(a.id as number);
    if (c.sudo) throw new Error("这条命令要管理员密码：复制到终端里自己跑");
    if (c.protected && !a.protectedOk) throw new Error("这条命令碰到受保护的地方，要再确认一次");
    c.state = "running";
    window.setTimeout(() => Object.assign(c, { state: "done", code: 0, output: `$ ${c.command}\n（假 Mac：这条命令没有真的跑，开发版和测试都不跑命令）\n` }), 1500);
    return structuredClone(c);
  },
  janitor_card_stop: (a) => {
    const c = findCard(a.id as number);
    if (c?.state === "running") Object.assign(c, { state: "stopped", output: "（停下了）" });
    return structuredClone(c);
  },
  janitor_card_rule: (a) => {
    const c = pendingCard(a.id as number);
    const r = { ...c.rule!, id: rules.length + 1, createdAt: new Date().toISOString() };
    rules.push(r);
    c.rule = r;
    c.state = "done";
    return structuredClone(c);
  },
  janitor_card_cancel: (a) => {
    const c = pendingCard(a.id as number);
    c.state = "cancelled";
    return structuredClone(c);
  },
};

/** The 模块坞's 「可清 N GB」. */
export const janitorLive = () => {
  const n = view().cleanable;
  return n > 0 ? `可清 ${(n / 1e9).toFixed(1)} GB` : null;
};

const CATEGORIES = [
  { id: "node_modules", name: "项目里的 node_modules", rebuild: "在项目里 pnpm install（或 npm install）", takes: "一两分钟", network: true, download: false },
  { id: "rust_target", name: "Rust 的 target", rebuild: "下次编译时重建", takes: "几分钟（Pomelo 约一分半）", network: false, download: false },
  { id: "python", name: "Python 虚拟环境和 __pycache__", rebuild: "按项目的依赖清单重新装；__pycache__ 运行时自己生成", takes: "几分钟", network: true, download: false },
  { id: "web_build", name: "前端的构建产物", rebuild: "下次构建时重建（npm run build）", takes: "几十秒", network: false, download: false },
  { id: "pkg_cache", name: "包管理器的下载缓存", rebuild: "用到时重新下载", takes: "看网速", network: true, download: true },
  { id: "conda", name: "Conda 的包缓存", rebuild: "conda clean 清掉没在用的包；以后装包时重新下载", takes: "看网速", network: true, download: true },
  { id: "jvm", name: "Gradle、Maven 的缓存", rebuild: "下次构建时重新下载", takes: "看网速", network: true, download: true },
  { id: "xcode", name: "Xcode 的编译缓存和旧文件", rebuild: "下次编译时重建；连上设备时重新准备", takes: "几分钟", network: true, download: false },
  { id: "app_cache", name: "各个 App 的缓存", rebuild: "App 用到时自己再生成", takes: "马上", network: false, download: false },
  { id: "codex", name: "Codex 的运行时", rebuild: "Codex 用到时重新下载", takes: "几分钟", network: true, download: false },
  { id: "logs", name: "日志", rebuild: "不用重建", takes: "", network: false, download: false },
];
