// 柚子链接's parts: the chip a `[[目标]]` is drawn as, the Pomelo link a `[[目标|字]]` is, opening what a link names,
// the 「柚子链接 · N」 lists (panels, a note's card, an article's end) and the `[[` suggestions (links.ts, links.rs).
import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from "react";
import { ModuleIcon } from "@/app/ModuleIcon";
import { ownMark, ownTint } from "@/app/looks";
import { useModuleLooks } from "@/app/moduleLooks";
import { sizedMark, stickerById, tintColor } from "@/shared/marks";
import { moduleById } from "@/app/modules";
import { useHiddenModules } from "@/app/hidden";
import { useTabs } from "@/app/tabs";
import { Brackets, ChevronDown, Feather } from "@/shared/icons";
import { Tip } from "@/shared/handParts";
import { Field } from "@/shared/ui";
import { Menu, type MenuAt } from "@/app/Menu";
import { ClipBox } from "./ClipBox";
import type { At } from "./LinkPicker";
import type { Hit } from "@/shared/open";
import { RECORD, dayText, foundIn, linksApi, shortDay, targetOf, useBacklinks, useLookup, type Backlink, type LinkModule, type Thing } from "./links";
import "./links.css";

/** Opens a thing in its module (the page picks it up with useOpenRequest). */
export function useOpenThing() {
  const tabs = useTabs();
  return useCallback(
    (t: { module: LinkModule; id: number; title: string }) => {
      const hit: Hit = { module: t.module, kind: RECORD[t.module], id: t.id, title: t.title, detail: null, parent: null, day: null, amount: null, currency: null, flow: null };
      tabs.open(`/${t.module}`, { open: hit });
    },
    [tabs],
  );
}

/** What a page's texts link to, looked up once for all of them (念念不忘's list); a text outside one looks up its own. */
const LookupCtx = createContext<Map<string, Thing | null> | null>(null);
export function LinkLookup(props: { texts: string[]; children: ReactNode }) {
  const map = useLookup(props.texts.flatMap((t) => (t.includes("[[") ? foundIn(t).map((f) => f.target) : [])));
  return <LookupCtx.Provider value={map}>{props.children}</LookupCtx.Provider>;
}
/** The things a text's links name (undefined: not known yet). */
export function useLinkThings(text: string): (target: string) => Thing | null | undefined {
  const shared = useContext(LookupCtx);
  const own = useLookup(shared || !text.includes("[[") ? [] : foundIn(text).map((f) => f.target));
  const map = shared ?? own;
  return (t) => map.get(t);
}

/** A module's sticker as HTML (what it wears, in its color), `size` px: for texts drawn as HTML (见字如面's editor). */
export function useStickerHtml(size = 16): (module: string) => string {
  const { eff } = useModuleLooks();
  const n = useRef(0);
  return useCallback(
    (module: string) => {
      const def = moduleById(module);
      const e = eff.get(module);
      const svg = ((e && stickerById(e.mark)) ?? stickerById(ownMark(def)))?.svg;
      if (!svg) return "";
      const tint = e ? e.tint : ownTint(def);
      const style = tint ? ` style="--mark-color:${tintColor(tint)};--mark-color-soft:var(--pm-cat-${tint}-sticker)"` : "";
      return `<span class="pm-mk"${style}>${sizedMark(svg, size, `lk${++n.current}`)}</span>`;
    },
    [eff, size],
  );
}

/** A module's sticker, small. */
export function LinkMark(props: { module: string; size?: number }) {
  return <ModuleIcon def={moduleById(props.module)} size={props.size ?? 16} className="pm-lk-mk" />;
}

/** `[[目标]]` (a chip: sticker + name) or `[[目标|字]]` (the words as a Pomelo link). `thing` undefined: not looked up
 *  yet; null: 找不到. */
export function LinkChip(props: { target: string; alias?: string | null; thing: Thing | null | undefined; size?: number }) {
  const open = useOpenThing();
  const { module, name } = targetOf(props.target);
  const dead = props.thing === null;
  const go = (e: React.MouseEvent) => {
    e.stopPropagation();
    if (props.thing) open(props.thing);
  };
  if (props.alias)
    return (
      <button type="button" onClick={go} className={`pm-lkw ${dead ? "dead" : ""}`} title={dead ? `找不到「${name}」` : undefined}>
        {props.alias}
      </button>
    );
  return (
    <button type="button" onClick={go} className={`pm-lk ${dead ? "dead" : ""}`}>
      <LinkMark module={module} size={props.size ?? 16} />
      <span>{props.thing?.module === "notes" ? props.thing.title : name}</span>
      {dead && <span className="pm-lk-gone">找不到</span>}
    </button>
  );
}

/** 「柚子链接 · N」 in a panel (书影游): one row each, the sticker, an article's title in bold or a note's opening words,
 *  the date. 「记一条」 beside the heading opens 念念不忘's box starting with a link to this one (not while 念念不忘 is
 *  hidden); with nothing linking here yet only that shows. */
export function BacklinkList(props: { module: string; id: number }) {
  const { data = [] } = useBacklinks(props.module, props.id);
  const open = useOpenThing();
  const [hidden] = useHiddenModules();
  const [jot, setJot] = useState<{ at: At; text: string } | null>(null);
  const notes = !hidden.includes("notes");
  if (!data.length && !notes) return null;
  const start = async (e: React.MouseEvent<HTMLElement>) => {
    const r = e.currentTarget.getBoundingClientRect();
    const t = await linksApi.thing(props.module, props.id).catch(() => null);
    if (t) setJot({ at: { left: Math.max(12, Math.min(r.right - 440, window.innerWidth - 452)), top: Math.max(12, Math.min(r.bottom + 8, window.innerHeight - 260)) }, text: `[[${t.target}]] ` });
  };
  return (
    <Field
      label={data.length ? `柚子链接 · ${data.length}` : "柚子链接"}
      action={
        notes ? (
          <button type="button" onClick={(e) => void start(e)} className="flex items-center gap-1 text-xs text-primary hover:underline">
            <Feather size={13} /> 记一条
          </button>
        ) : undefined
      }
    >
      {jot && <ClipBox at={jot.at} text={jot.text} label="记一条" onClose={() => setJot(null)} />}
      <div className="pm-bkl">
        {data.map((b) => (
          <button key={`${b.module}${b.id}`} type="button" className="pm-bkr" onClick={() => open(b)}>
            <LinkMark module={b.module} size={20} />
            <span className={`min-w-0 flex-1 truncate ${b.module === "articles" ? "font-bold" : ""}`}>{b.title || "无标题"}</span>
            <span className="shrink-0 text-xs text-muted-foreground tabular-nums">{shortDay(b.date)}</span>
          </button>
        ))}
      </div>
    </Field>
  );
}

/** 「柚子链接 · N」 over an article (its head, beside 导出): the count; a click lists them, a row opens one. Nothing
 *  while nothing links here. The same ones stand as chips after the text (BacklinkChips). */
export function BacklinkButton(props: { module: string; id: number }) {
  const { data = [] } = useBacklinks(props.module, props.id);
  const open = useOpenThing();
  const [menu, setMenu] = useState<MenuAt | null>(null);
  if (!data.length) return null;
  const short = (s: string) => ([...s].length > 26 ? `${[...s].slice(0, 26).join("")}…` : s);
  return (
    <>
      <button
        type="button"
        aria-label={`柚子链接 ${data.length}`}
        aria-haspopup="menu"
        onClick={(e) => {
          const r = e.currentTarget.getBoundingClientRect();
          setMenu({ x: r.left, y: r.bottom + 6, rows: [{ head: `柚子链接 · ${data.length}` }, ...data.map((b) => ({ label: short(b.title || "无标题"), icon: <LinkMark module={b.module} size={14} />, onSelect: () => open(b) }))] });
        }}
        className="pm-bkb pm-press pm-tip-host relative inline-flex shrink-0 items-center gap-1 rounded-control p-1.5 text-xs tabular-nums transition-colors duration-150 outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring"
      >
        <Brackets size={16} />
        {data.length}
        <Tip>柚子链接</Tip>
      </button>
      {menu && <Menu {...menu} onClose={() => setMenu(null)} />}
    </>
  );
}

/** 「柚子链接 · N」 at an article's end: a row of chips (sticker, title, date). */
export function BacklinkChips(props: { items: Backlink[] }) {
  const open = useOpenThing();
  if (!props.items.length) return null;
  return (
    <div className="pm-bkx">
      <p className="pm-bkh">柚子链接 · {props.items.length}</p>
      <div className="flex flex-wrap gap-2">
        {props.items.map((b) => (
          <button key={`${b.module}${b.id}`} type="button" className="pm-lk pm-lk-lg" onClick={() => open(b)}>
            <LinkMark module={b.module} size={16} />
            <span className="max-w-72 truncate">{b.title || "无标题"}</span>
            <span className="pm-lk-d">{shortDay(b.date)}</span>
          </button>
        ))}
      </div>
    </div>
  );
}

/** 「柚子链接 · N」 under a note's card: folded to the count, opens into rows. */
export function BacklinkFold(props: { items: Backlink[]; initiallyOpen?: boolean }) {
  const [open, setOpen] = useState(!!props.initiallyOpen);
  const go = useOpenThing();
  if (!props.items.length) return null;
  return (
    <div className="mt-2">
      <button type="button" onClick={() => setOpen(!open)} aria-expanded={open} className="flex h-5 items-center gap-1 text-xs text-muted-foreground hover:text-foreground">
        柚子链接 · {props.items.length}
        <ChevronDown size={12} className={`transition-transform duration-150 ${open ? "" : "-rotate-90"}`} />
      </button>
      {open &&
        props.items.map((b) => (
          <button key={`${b.module}${b.id}`} type="button" onClick={() => go(b)} className="flex h-7 w-full min-w-0 items-center gap-1.5 text-left text-[13px] text-muted-foreground hover:text-foreground">
            <LinkMark module={b.module} size={16} />
            <span className={`min-w-0 truncate ${b.module === "articles" ? "font-bold text-foreground" : ""}`}>{b.title || "无标题"}</span>
            <span className="shrink-0 text-xs tabular-nums">{shortDay(b.date)}</span>
          </button>
        ))}
    </div>
  );
}

// ---------- [[ : what to offer ----------

const TTL = 15_000;
const KEY = "\u0002";

/**
 * The `[[` list for a LiveText `Suggest`: `items(q)` answers at once from what Rust last sent for `q` (asking again
 * when it's old), and `version` changes when an answer comes, so the host rebuilds its Suggest and the list redraws.
 * `except`: the article or note being written.
 */
export function useLinkSource(except: [LinkModule, number] | null) {
  const [hidden] = useHiddenModules();
  const cache = useRef(new Map<string, { at: number; things: Thing[] }>());
  const asking = useRef(new Set<string>());
  const things = useRef(new Map<string, Thing>());
  const query = useRef("");
  const last = useRef<Thing[]>([]);
  const [version, setVersion] = useState(0);
  const exceptKey = except ? except.join("/") : "";
  const hiddenKey = hidden.join(",");
  useEffect(() => {
    cache.current.clear();
  }, [exceptKey, hiddenKey]);
  const keyOf = (t: Thing) => `${KEY}${t.module}:${t.id}`;
  const items = useCallback(
    (q: string): string[] => {
      query.current = q;
      const hit = cache.current.get(q);
      if ((!hit || Date.now() - hit.at > TTL) && !asking.current.has(q)) {
        asking.current.add(q);
        void linksApi
          .suggest(q, hidden, except)
          .then((r) => {
            cache.current.set(q, { at: Date.now(), things: r });
            r.forEach((t) => things.current.set(keyOf(t), t));
            setVersion((v) => v + 1);
          })
          .catch(() => {})
          .finally(() => asking.current.delete(q));
      }
      const shown = hit?.things ?? last.current;
      last.current = shown;
      return shown.map(keyOf);
    },
    [hiddenKey, exceptKey], // eslint-disable-line react-hooks/exhaustive-deps
  );
  const thing = useCallback((key: string) => things.current.get(key) ?? null, []);
  const isLink = (key: string) => key.startsWith(KEY);
  /** A line of the list: sticker, the name (what's typed marked), a soft line (作者、平台 · 状态、日期). */
  const show = (key: string): ReactNode => {
    const t = things.current.get(key);
    if (!t) return null;
    const q = query.current.trim();
    const at = q ? t.title.toLowerCase().indexOf(q.toLowerCase()) : -1;
    const name = at < 0 ? t.title : (
      <>
        {t.title.slice(0, at)}
        <em className="pm-sug-hit">{t.title.slice(at, at + q.length)}</em>
        {t.title.slice(at + q.length)}
      </>
    );
    const meta = t.module === "articles" || t.module === "notes" ? dayText(t.date) : t.meta;
    return (
      <span className="pm-sug">
        <LinkMark module={t.module} size={24} />
        <span className="pm-sug-t">
          <span className="pm-sug-n">{name || "无标题"}</span>
          {meta && <span className="pm-sug-m">{meta}</span>}
        </span>
      </span>
    );
  };
  return { items, thing, show, isLink, version };
}

/** What `[[` inserts for a picked thing (the closing `]]` unless it's already there). */
export const linkInsert = (t: Thing, after: string) => `[[${t.target}${after.startsWith("]]") ? "" : "]]"}`;

/** A `[[` being typed right before the caret (not in `![[`… the editor also takes that): where `[[` starts, the query. */
export function typingLink(value: string, caret: number): { start: number; query: string } | null {
  const w = /\[\[([^\]\n[|]*)$/.exec(value.slice(0, caret));
  return w ? { start: caret - w[1].length - 2, query: w[1] } : null;
}
