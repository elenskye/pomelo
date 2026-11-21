// 见字如面 (after Bear): the whole page is three columns, no title and no card around them: the six fixed lists and the
// tag tree (念念不忘's, its own tags), the list, the article. The two lines between them drag the widths (each column has
// a minimum and stops there); two fingers sideways anywhere on the page fold the tags, then the list (and back), the
// columns sliding away; over a row they draw it aside over its delete (swipe.ts); a narrow window shows two.
// Several are picked with ⌘-click / ⇧-click (Esc lets go): the right side then offers to export them. A tag's 「⋯」 and
// 设置 export a tag / all; the head exports the open one (Transfer.tsx). 设置 also has 从 Bear 搬家 and 导入.
// 专心写 fills the window (the top bar too) with the article and 「缩小」: the article is always drawn into one node of
// its own (a portal), which moves between the card and <body> — the box stays mounted (caret, its ⌘Z). A new article is written straight away and
// kept from its first word: it starts as an empty first-level heading (the caret there) and, in a tag view, that tag on
// the second line. 「设置」 is the module's second page (纸签), the only one with a title.
import { useEffect, useLayoutEffect, useMemo, useRef, useState, type CSSProperties, type PointerEvent as ReactPointerEvent, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { useQueryClient } from "@tanstack/react-query";
import { moduleById } from "@/app/modules";
import { usePageContext } from "@/app/pageContext";
import { useSubNav, useSubPage } from "@/app/subpage";
import { Dropdown } from "@/shared/Dropdown";
import { fileSrc } from "@/shared/attachments/api";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { Archive, ArchiveRestore, Ellipsis, Feather, FileOutput, Lock, Maximize2, Minimize2, Notebook, Pin, Sparkles, SquarePen, TagOff, Trash2 } from "@/shared/icons";
import { useOpenRequest } from "@/shared/open";
import { usePageKeys } from "@/shared/panel";
import { useSetting } from "@/shared/settings";
import { Button, IconButton, PageHeader, Picked, SearchInput, Segmented } from "@/shared/ui";
import { TagTree, type TagScope } from "@/modules/notes/TagTree";
import { Unlock } from "@/modules/vault/Gate";
import { VAULT_STATUS_KEY, useVaultStatus, vaultApi } from "@/modules/vault/api";
import { ARTICLES_KEY, listDate, useArticle, useArticleMutations, useArticles, useArticlesOverview, useSimilarArticles, useTitles, type Article, type Filter, type Summary, type View } from "./api";
import { ArticleEditor, type BarMode, type EditorHandle, type Look2 } from "./Editor";
import { drawDoc, type Look } from "./bear";
import { BearDialog, ExportDialog, useImportFiles } from "./Transfer";
import type { ExportTarget } from "./api";
import { useSwipes } from "./swipe";
import { Shelf } from "./Shelf";
import { BacklinkButton } from "@/shared/links/LinkParts";
import { Menu, type MenuAt, type MenuRow } from "@/app/Menu";
import "./articles.css";

const MOD = "articles";
const BASE_SCOPE: TagScope = {
  useMutations: useArticleMutations,
  openKey: "articles.tags_open",
  color: "var(--module-articles)",
  unit: "篇笔记",
  withItems: { option: "连笔记一起移到废纸篓", button: (n) => `把 ${n} 篇移到废纸篓`, done: (n) => `已把 ${n} 篇移到废纸篓`, explain: (n) => `用到它和子标签的 ${n} 篇都移到废纸篓，能捡回来。` },
};

const VIEWS: { id: View; label: string; icon: typeof Notebook }[] = [
  { id: "all", label: "全部笔记", icon: Notebook },
  { id: "pinned", label: "置顶", icon: Pin },
  { id: "untagged", label: "没标签的", icon: TagOff },
  { id: "archived", label: "归档", icon: Archive },
  { id: "trash", label: "废纸篓", icon: Trash2 },
  { id: "locked", label: "加密的", icon: Lock },
];

type Cols = { t: number; l: number };
const isCols = (v: unknown): v is Cols => !!v && typeof (v as Cols).t === "number" && typeof (v as Cols).l === "number";
const MIN = { t: 150, l: 220, e: 380 };
const MAX = { t: 300, l: 440 };

/** Bear's spacing (its default, measured): 17 px, lines 1.9 apart, a fifth of the size after each line, blank lines whole. */
export const DEFAULT_LOOK: Look2 = { size: 17, leading: 1.9, para: 0.2, width: 680, bar: "always" };
const isLook = (v: unknown): v is Look2 => {
  const l = v as Look2;
  return !!l && typeof l.size === "number" && typeof l.leading === "number" && (l.para === undefined || typeof l.para === "number") && (l.width === null || typeof l.width === "number") && (l.bar === "always" || l.bar === "select");
};
/** A look saved before 段距 existed gets the default one. */
export function useLook() {
  const [look, setLook] = useSetting<Look2>("articles.look", DEFAULT_LOOK, isLook);
  return [{ ...look, para: look.para ?? DEFAULT_LOOK.para }, setLook] as const;
}
/** 正文宽度: 480–1120 px; one step past the end is 占满 (null). */
const WIDTH = { min: 480, max: 1160, step: 40 };

// ---------- 设置 ----------

function Row(props: { label: string; children: ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-6 py-3">
      <span className="text-sm">{props.label}</span>
      {props.children}
    </div>
  );
}

/** A setting dragged along a line (Bear's typography), its value beside it. */
function Slider(props: { label: string; value: number; min: number; max: number; step: number; show: (v: number) => string; onChange: (v: number) => void }) {
  const p = ((props.value - props.min) / (props.max - props.min)) * 100;
  return (
    <div className="flex items-center gap-3">
      <input
        type="range"
        aria-label={props.label}
        className="ar-range"
        min={props.min}
        max={props.max}
        step={props.step}
        value={props.value}
        onChange={(e) => props.onChange(Number(e.target.value))}
        style={{ "--p": `${Math.max(0, Math.min(100, p))}%` } as CSSProperties}
      />
      <span className="w-11 text-right text-sm text-muted-foreground tabular-nums">{props.show(props.value)}</span>
    </div>
  );
}

const SAMPLE = "# 东京的第三天\n早上被楼下面包店的香味叫醒。今天的计划是谷中银座和根津，走累了就找一家咖啡店坐下来，把昨天的照片挑一挑。\n\n下午下了一阵雨，躲进一家旧书店，买到一本 1987 年的旅行手册。\n- 谷中银座的炸肉饼\n- 根津神社的千本鸟居";
const SAMPLE_LOOK: Look = { image: () => null, file: () => false, thing: () => undefined, sticker: () => "", math: () => null, mermaid: () => null, code: () => null, folds: new Set() };

function Settings() {
  const [saved, setLook] = useLook();
  // dragging shows at once and is kept a moment after it stops
  const [draft, setDraft] = useState<Look2 | null>(null);
  const look = draft ?? saved;
  const keep = useRef(0);
  const set = (p: Partial<Look2>) => {
    const next = { ...look, ...p };
    setDraft(next);
    window.clearTimeout(keep.current);
    keep.current = window.setTimeout(() => {
      setLook(next);
      setDraft(null);
    }, 250);
  };
  useEffect(() => () => window.clearTimeout(keep.current), []);
  const sample = useMemo(() => drawDoc(SAMPLE, null, null, SAMPLE_LOOK).join(""), []);
  const isDefault = look.size === DEFAULT_LOOK.size && look.leading === DEFAULT_LOOK.leading && look.para === DEFAULT_LOOK.para && look.width === DEFAULT_LOOK.width;
  const [bear, setBear] = useState(false);
  const [exporting, setExporting] = useState(false);
  const [importFiles, importDialog] = useImportFiles();
  const all = useArticlesOverview().data?.counts;
  return (
    <div className="-m-1 flex min-h-0 flex-1 gap-10 overflow-y-auto p-1 px-10 pb-8">
      <div className="w-[420px] shrink-0 divide-y divide-dashed">
        <Row label="正文字号">
          <Slider label="正文字号" value={look.size} min={13} max={24} step={1} show={(v) => String(v)} onChange={(size) => set({ size })} />
        </Row>
        <Row label="行距">
          <Slider label="行距" value={look.leading} min={1.3} max={2.6} step={0.05} show={(v) => v.toFixed(2)} onChange={(leading) => set({ leading })} />
        </Row>
        <Row label="段距">
          <Slider label="段距" value={look.para} min={0} max={1.2} step={0.05} show={(v) => v.toFixed(2)} onChange={(para) => set({ para })} />
        </Row>
        <Row label="正文宽度">
          <Slider label="正文宽度" value={look.width ?? WIDTH.max} min={WIDTH.min} max={WIDTH.max} step={WIDTH.step} show={(v) => (v >= WIDTH.max ? "占满" : String(v))} onChange={(v) => set({ width: v >= WIDTH.max ? null : v })} />
        </Row>
        <Row label="排版">
          <Button disabled={isDefault} onClick={() => set({ size: DEFAULT_LOOK.size, leading: DEFAULT_LOOK.leading, para: DEFAULT_LOOK.para, width: DEFAULT_LOOK.width })}>
            恢复默认
          </Button>
        </Row>
        <Row label="格式条">
          <Segmented<BarMode> ariaLabel="格式条" value={look.bar} onChange={(bar) => set({ bar })} options={[{ value: "always", label: "一直在" }, { value: "select", label: "选中文字时才出来" }]} />
        </Row>
        <Row label="从 Bear 搬家">
          <Button onClick={() => setBear(true)}>开始…</Button>
        </Row>
        <Row label="导入 Markdown、TextBundle">
          <div className="flex gap-2">
            <Button onClick={() => void importFiles(false)}>文件…</Button>
            <Button onClick={() => void importFiles(true)}>文件夹…</Button>
          </div>
        </Row>
        <Row label="导出全部">
          <Button disabled={!all || all.all + all.archived === 0} onClick={() => setExporting(true)}>
            导出…
          </Button>
        </Row>
        {bear && <BearDialog onClose={() => setBear(false)} />}
        {exporting && all && <ExportDialog target={{ all: true }} label={`全部 · ${all.all + all.archived} 篇`} onClose={() => setExporting(false)} />}
        {importDialog}
      </div>
      <div className="pm-card min-w-0 flex-1 self-start !py-5 !pr-6" data-hand="card" data-seed={1461}>
        <div className="ar-sample" style={{ "--ar-size": `${look.size}px`, "--ar-leading": look.leading, "--ar-para": `${look.para}em`, maxWidth: look.width ?? undefined, paddingLeft: `calc(${look.size}px * 2.7)` } as CSSProperties}>
          <div className="ar-text !min-h-0" aria-hidden dangerouslySetInnerHTML={{ __html: sample }} />
        </div>
      </div>
    </div>
  );
}

// ---------- the list ----------

/** A row; two fingers to the left draw it aside over its delete (swipe.ts). */
function ListRow(props: { a: Summary; on: boolean; trash: boolean; onOpen: (e: React.MouseEvent) => void; onMenu: (e: React.MouseEvent) => void }) {
  const a = props.a;
  return (
    <div className={`ar-swipe ${props.on ? "on" : ""}`} data-id={a.id} onContextMenu={props.onMenu}>
      <button type="button" tabIndex={-1} aria-label={props.trash ? "彻底删除" : "移到废纸篓"} className="ar-swipe-del">
        <span className="ar-swipe-pill">
          <Trash2 size={17} />
        </span>
      </button>
    <button type="button" aria-current={props.on} onClick={props.onOpen} className={`ar-row ${props.on ? "on" : ""}`}>
      <span className="min-w-0">
        <span className="ar-row-title flex items-center gap-1.5">
          {a.pinned && !a.trashed && <Pin size={14} className="shrink-0 text-[var(--pm-pen)]" />}
          {a.locked && <Lock size={14} className="shrink-0 text-[var(--pm-pen)]" />}
          <span className="truncate">{a.title || "无标题"}</span>
        </span>
        <span className="ar-row-preview">{a.locked ? "锁着" : a.preview}</span>
        <span className="ar-row-date block">{listDate(a.updatedAt)}</span>
      </span>
      {a.thumb !== null && <img src={fileSrc(a.thumb)} alt="" className="ar-thumb" draggable={false} />}
    </button>
    </div>
  );
}

// ---------- the page ----------

export default function ArticlesPage() {
  const [sub, setSub] = useSubPage<"write" | "settings">(MOD);
  useSubNav(MOD, (p) => setSub(p as "write" | "settings"));
  // an article opened from elsewhere (⌘K, Atlas, 柚子链接) comes up in 写 even when the settings page is showing
  const [asked, setAsked] = useState<{ id: number } | null>(null);
  useOpenRequest((hit) => {
    setSub("write");
    setAsked({ id: hit.id });
  });
  const mod = moduleById(MOD);
  return (
    <div className="relative flex h-full min-h-0 flex-col overflow-hidden">
      {sub === "settings" && <PageHeader icon={mod.icon} color={mod.color} title={`${mod.label} · 设置`} />}
      {sub === "settings" ? <Settings /> : <Writing asked={asked} onTaken={() => setAsked(null)} />}
    </div>
  );
}

function Writing({ asked, onTaken }: { asked: { id: number } | null; onTaken: () => void }) {
  const qc = useQueryClient();
  const toast = useToast();
  const [confirm, confirmDialog] = useConfirm();
  const m = useArticleMutations();
  const [look, setLook] = useLook();
  const [cols, setCols] = useSetting<Cols>("articles.cols", { t: 196, l: 300 }, isCols);
  const [w, setW] = useState(cols);
  useEffect(() => setW(cols), [cols]);
  const [view, setView] = useState<View>("all");
  const [tag, setTag] = useState<string | null>(null);
  const [search, setSearch] = useState("");
  const [q, setQ] = useState("");
  const [sel, setSel] = useState<number | null>(null);
  const [fresh, setFresh] = useState<{ n: number; seed: string } | null>(null);
  const [editorKey, setEditorKey] = useState(0);
  const [fold, setFold] = useState(0);
  /** 书架 (念念不忘 at the right while writing): opening it folds the tags away, closing puts back what was. */
  const [shelf, setShelf] = useState<{ fold: number } | null>(null);
  const toggleShelf = () => {
    if (shelf) {
      setFold(shelf.fold);
      setShelf(null);
    } else {
      setShelf({ fold });
      setFold((x) => Math.max(1, x));
    }
  };
  const [narrow, setNarrow] = useState(false);
  const [focus, setFocus] = useState(false);
  const [drag, setDrag] = useState<"t" | "l" | null>(null);
  /** ⌘ / ⇧-clicked (two or more: the right side offers to export them). */
  const [picked, setPicked] = useState<number[]>([]);
  const [exporting, setExporting] = useState<{ target: ExportTarget; label: string } | null>(null);
  const editor = useRef<EditorHandle>(null);
  const colsBox = useRef<HTMLDivElement>(null);
  const slot = useRef<HTMLElement>(null);
  const host = useMemo(() => document.createElement("section"), []);
  useLayoutEffect(() => {
    host.setAttribute("aria-label", "正文");
    host.className = focus ? "ar-focus" : "ar-main";
    // only when it moves: appending it where it already is takes the editor out and puts it back, and a two-finger
    // swipe over it then starts again (the columns folded twice)
    const to = focus ? document.body : slot.current;
    if (to && host.parentNode !== to) to.appendChild(host);
  }, [focus, host, fold, narrow]);
  useEffect(() => () => host.remove(), [host]);
  const searchRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    const t = setTimeout(() => setQ(search.trim()), 200);
    return () => clearTimeout(t);
  }, [search]);
  const filter: Filter = useMemo(() => {
    const f: Filter = { view: tag ? "all" : view };
    if (tag) f.tag = tag;
    if (q) f.q = q;
    return f;
  }, [view, tag, q]);
  const listQ = useArticles(filter);
  const list = listQ.data ?? [];
  const similar = useSimilarArticles(filter, q !== "");
  const near = q ? (similar.data ?? []).filter((n) => !list.some((x) => x.id === n.id)) : [];
  const overview = useArticlesOverview().data;
  const titles = useTitles().data ?? [];
  const tagNames = useMemo(() => (overview?.tags ?? []).map((t) => t.tag), [overview]);
  const scope: TagScope = useMemo(
    () => ({ ...BASE_SCOPE, actions: [{ value: "export", label: "导出…", run: (t: string) => setExporting({ target: { tag: t }, label: `#${t} · ${overview?.tags.find((x) => x.tag === t)?.count ?? 0} 篇` }) }] }),
    [overview],
  );
  const articleQ = useArticle(sel);
  const article = articleQ.data ?? null;
  const vault = useVaultStatus().data;

  // the first article of a list opens by itself
  useEffect(() => {
    if (sel === null && !fresh && list.length && listQ.isFetched) setSel(list[0].id);
  }, [list, sel, fresh, listQ.isFetched]);
  // a locked article shuts again when 芝麻开门 locks itself (5 minutes idle); opens when it opens
  useEffect(() => {
    if (!article?.locked) return;
    const t = window.setInterval(() => void qc.invalidateQueries({ queryKey: VAULT_STATUS_KEY }), 30_000);
    return () => window.clearInterval(t);
  }, [article?.locked]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => {
    if (article?.locked && vault && (vault.state === "open") !== article.open) void qc.invalidateQueries({ queryKey: [...ARTICLES_KEY, "one", article.id] });
  }, [vault?.state]); // eslint-disable-line react-hooks/exhaustive-deps

  const open = async (id: number | null) => {
    await editor.current?.flush();
    setPicked([]);
    setFresh(null);
    setSel(id);
    setEditorKey((k) => k + 1);
  };
  const create = async () => {
    await editor.current?.flush();
    setPicked([]);
    setSel(null);
    setFresh({ n: Date.now(), seed: tag ? `# \n#${tag}` : "# " });
    setEditorKey((k) => k + 1);
  };
  const go = async (v: View | null, t: string | null = null) => {
    await editor.current?.flush();
    setPicked([]);
    setView(v ?? "all");
    setTag(t);
    setFresh(null);
    setSel(null);
    setEditorKey((k) => k + 1);
  };

  // ⌘-click adds or takes one away, ⇧-click takes the run from the open one; a plain click opens
  const rows = [...list, ...near];
  const rowsRef = useRef<Summary[]>([]);
  rowsRef.current = rows;
  const selRef = useRef(sel);
  selRef.current = sel;
  const click = (id: number) => async (e: React.MouseEvent) => {
    if (e.metaKey || e.shiftKey) {
      await editor.current?.flush();
      const base = picked.length ? picked : sel !== null ? [sel] : [];
      if (e.shiftKey && sel !== null) {
        const ids = rows.map((r) => r.id);
        const [a, b] = [ids.indexOf(sel), ids.indexOf(id)];
        if (a >= 0 && b >= 0) return setPicked(ids.slice(Math.min(a, b), Math.max(a, b) + 1));
      }
      return setPicked(base.includes(id) ? base.filter((x) => x !== id) : [...base, id]);
    }
    void open(id);
  };
  const several = picked.length > 1;
  useEffect(() => setPicked([]), [q]);

  useEffect(() => {
    if (!asked) return;
    onTaken();
    void open(asked.id);
  }, [asked]); // eslint-disable-line react-hooks/exhaustive-deps
  usePageKeys({
    onNew: () => void create(),
    onFind: () => searchRef.current?.focus(),
    onEscape: focus ? () => setFocus(false) : several ? () => setPicked([]) : undefined,
  });
  usePageContext({
    module: MOD,
    filter: [tag ? `#${tag}` : VIEWS.find((x) => x.id === view)?.label, q ? `搜「${q}」` : null].filter(Boolean).join(" · ") || null,
    record: article ? { kind: "article", id: article.id, label: `文章：${article.title || "无标题"}` } : null,
  });

  // ---------- widths, folding, narrow ----------
  useEffect(() => {
    const el = colsBox.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setNarrow(el.clientWidth < 940));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  const folded = Math.max(fold, narrow ? 1 : 0);
  const startDrag = (which: "t" | "l") => (e: ReactPointerEvent<HTMLDivElement>) => {
    e.preventDefault();
    const g = e.currentTarget;
    g.setPointerCapture(e.pointerId);
    const x0 = e.clientX;
    const w0 = { ...w };
    const total = colsBox.current?.clientWidth ?? 1200;
    let now = w0;
    setDrag(which);
    const move = (ev: PointerEvent) => {
      const dx = ev.clientX - x0;
      now =
        which === "t"
          ? { ...w0, t: Math.max(MIN.t, Math.min(MAX.t, w0.t + dx, total - 24 - w0.l - MIN.e)) }
          : { ...w0, l: Math.max(MIN.l, Math.min(MAX.l, w0.l + dx, total - (folded ? 0 : w0.t + 12) - 24 - MIN.e)) };
      setW(now);
    };
    const up = () => {
      g.removeEventListener("pointermove", move);
      g.removeEventListener("pointerup", up);
      g.removeEventListener("pointercancel", up);
      setDrag(null);
      setCols(now);
    };
    g.addEventListener("pointermove", move);
    g.addEventListener("pointerup", up);
    g.addEventListener("pointercancel", up);
  };
  // two fingers sideways anywhere on the page: left folds (tags, then the list), right unfolds; over a row, its delete
  useSwipes({
    cols: colsBox,
    onFold: (step) => setFold((f) => Math.max(0, Math.min(2, f + step))),
    onDelete: async (id) => {
      const a = rowsRef.current.find((x) => x.id === id);
      if (!a) return null;
      if (a.trashed) {
        if (!(await confirm({ title: "彻底删除这篇？", message: `「${a.title || "无标题"}」`, confirmLabel: "彻底删除", danger: true }))) return null;
        return async () => {
          await m.remove.mutateAsync(id);
          if (id === selRef.current) void open(null);
        };
      }
      if (id === selRef.current) await editor.current?.flush();
      return async () => {
        await m.trash.mutateAsync(id);
        toast("已移到废纸篓");
        if (id === selRef.current) void open(null);
      };
    },
  });

  // ---------- the article's actions ----------
  const act = async (a: { id: number; title: string }, what: string) => {
    try {
      if (what === "pin" || what === "unpin") await m.update.mutateAsync({ id: a.id, patch: { pinned: what === "pin" } });
      if (what === "archive" || what === "unarchive") {
        await editor.current?.flush();
        await m.update.mutateAsync({ id: a.id, patch: { archived: what === "archive" } });
        toast(what === "archive" ? "已归档" : "已放回全部笔记");
      }
      if (what === "lock" || what === "unlock") {
        await editor.current?.flush();
        const status = await vaultApi.status();
        if (status.state === "new") return toast("先在芝麻开门里设好主密码", "error");
        if (status.state !== "open") {
          const s = await vaultApi.unlockTouch().catch((e) => (toast(errorText(e), "error"), null));
          if (!s || s.state !== "open") return;
          void qc.invalidateQueries({ queryKey: VAULT_STATUS_KEY });
        }
        await m.lock.mutateAsync({ id: a.id, lock: what === "lock" });
        toast(what === "lock" ? "加密了：芝麻开门锁上时它也锁上" : "已取消加密");
      }
      if (what === "trash") {
        await editor.current?.flush();
        await m.trash.mutateAsync(a.id);
        toast("已移到废纸篓");
        if (a.id === selRef.current) void open(null);
      }
      if (what === "restore") {
        await m.restore.mutateAsync(a.id);
        toast("已捡回来");
      }
      if (what === "delete") {
        if (!(await confirm({ title: "彻底删除这篇？", message: `「${a.title || "无标题"}」`, confirmLabel: "彻底删除", danger: true }))) return;
        await m.remove.mutateAsync(a.id);
        if (a.id === selRef.current) void open(null);
      }
    } catch {
      // the mutation's own toast said it
    }
  };
  // right-click on a row: what the article's own buttons and 「⋯」 offer
  const [menu, setMenu] = useState<MenuAt | null>(null);
  const rowMenu = (a: Summary) => (e: React.MouseEvent) => {
    e.preventDefault();
    const several = picked.length > 1 && picked.includes(a.id);
    const rows: MenuRow[] = several
      ? [{ label: `导出选中的 ${picked.length} 篇…`, icon: <FileOutput size={14} />, onSelect: () => setExporting({ target: { ids: rows0().filter((x) => picked.includes(x)) }, label: `选中的 ${picked.length} 篇` }) }]
      : a.trashed
        ? [
            { label: "捡回来", icon: <ArchiveRestore size={14} />, onSelect: () => void act(a, "restore") },
            { label: "彻底删除…", icon: <Trash2 size={14} />, onSelect: () => void act(a, "delete") },
          ]
        : [
            { label: a.pinned ? "取消置顶" : "置顶", icon: <Pin size={14} />, onSelect: () => void act(a, a.pinned ? "unpin" : "pin") },
            {
              label: "导出…",
              icon: <FileOutput size={14} />,
              onSelect: async () => {
                if (a.id === selRef.current) await editor.current?.flush();
                setExporting({ target: { ids: [a.id] }, label: `「${a.title || "无标题"}」· 1 篇` });
              },
            },
            "-",
            { label: a.archived ? "放回全部笔记" : "归档", icon: <Archive size={14} />, onSelect: () => void act(a, a.archived ? "unarchive" : "archive") },
            { label: a.locked ? "取消加密" : "加密（Touch ID）", icon: <Lock size={14} />, onSelect: () => void act(a, a.locked ? "unlock" : "lock") },
            "-",
            { label: "移到废纸篓", icon: <Trash2 size={14} />, onSelect: () => void act(a, "trash") },
          ];
    setMenu({ x: e.clientX, y: e.clientY, rows });
  };
  const rows0 = () => rows.map((r) => r.id);
  const emptyTrash = async () => {
    if (!(await confirm({ title: "清空废纸篓？", message: `${overview?.counts.trash ?? 0} 篇。`, confirmLabel: "清空", danger: true }))) return;
    await m.emptyTrash.mutateAsync();
    void open(null);
  };

  const navRow = (v: (typeof VIEWS)[number]) => {
    const on = !tag && view === v.id;
    const Icon = v.icon;
    return (
      <button
        key={v.id}
        type="button"
        aria-current={on}
        onClick={() => void go(v.id)}
        className={`side-row w-full shrink-0 gap-2 px-2.5 text-left outline-none focus-visible:ring-2 focus-visible:ring-ring ${on ? "on" : ""}`}
      >
        {on && <Picked seed={1440 + VIEWS.indexOf(v)} />}
        <Icon size={15} strokeWidth={1.75} className="shrink-0 text-muted-foreground" />
        <span className="min-w-0 flex-1 truncate">{v.label}</span>
        <span className="text-xs font-normal text-muted-foreground tabular-nums">{overview?.counts[v.id] ?? ""}</span>
      </button>
    );
  };

  const head = (a: Article | null) => (
    <div className="ar-head">
      {a && !a.trashedAt && <BacklinkButton module="articles" id={a.id} />}
      {a && !a.trashedAt && (
        <IconButton
          label="导出"
          onClick={async () => {
            await editor.current?.flush();
            setExporting({ target: { ids: [a.id] }, label: `「${a.title || "无标题"}」· 1 篇` });
          }}
        >
          <FileOutput size={16} />
        </IconButton>
      )}
      {a && !a.trashedAt && (
        <IconButton label={a.pinned ? "取消置顶" : "置顶"} onClick={() => void act(a, a.pinned ? "unpin" : "pin")}>
          <Pin size={16} className={a.pinned ? "fill-current" : ""} />
        </IconButton>
      )}
      {a?.trashedAt && (
        <>
          <Button onClick={() => void act(a, "restore")}>
            <ArchiveRestore size={14} /> 捡回来
          </Button>
          <Button variant="ghost" className="text-destructive" onClick={() => void act(a, "delete")}>
            彻底删除
          </Button>
        </>
      )}
      {a && !a.trashedAt && (
        <Dropdown<string | null>
          plain
          ariaLabel="这篇的更多操作"
          value={null}
          align="end"
          onChange={(v) => v && void act(a, v)}
          options={[
            { value: a.archived ? "unarchive" : "archive", label: a.archived ? "放回全部笔记" : "归档" },
            { value: a.locked ? "unlock" : "lock", label: a.locked ? "取消加密" : "加密（Touch ID）" },
            { value: "trash", label: "移到废纸篓" },
          ]}
          renderTrigger={() => <Ellipsis size={16} />}
          triggerClassName="pm-tip-host rounded-control p-1.5 text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
        />
      )}
      {a && !a.trashedAt && !focus && (
        <IconButton label="念念不忘" onClick={toggleShelf} on={!!shelf}>
          <Feather size={16} />
        </IconButton>
      )}
      <IconButton label={focus ? "缩小" : "专心写"} onClick={() => setFocus(!focus)}>
        {focus ? <Minimize2 size={16} /> : <Maximize2 size={16} />}
      </IconButton>
    </div>
  );

  const showEditor = fresh !== null || (article !== null && article.id === sel);
  const lockedShut = article?.locked && !article.open;
  const empty = listQ.isFetched && list.length === 0 && near.length === 0;
  return (
    <div ref={colsBox} className={`ar-cols ${drag ? "dragging cursor-col-resize select-none" : ""}`}>
      <div className={`ar-fold -m-1 p-1 ${folded >= 1 ? "shut" : ""}`} style={{ width: folded >= 1 ? 0 : w.t + 14 }} inert={folded >= 1}>
          <nav aria-label="见字如面的分类" className="ar-nav -m-1 overflow-y-auto p-1 pr-1" style={{ width: w.t }}>
            <div className="space-y-0.5">{VIEWS.map(navRow)}</div>
            {tagNames.length > 0 && <TagTree tags={overview?.tags ?? []} pinned={overview?.pinned ?? []} active={tag} onSelect={(t) => void go(null, t)} scope={scope} />}
          </nav>
          <div className={`ar-grip ${drag === "t" ? "on" : ""}`} role="separator" aria-orientation="vertical" aria-label="调宽标签栏" onPointerDown={startDrag("t")} />
      </div>
      <div className="ar-card">
        <div className={`ar-fold -m-1 p-1 ${folded >= 2 ? "shut" : ""}`} style={{ width: folded >= 2 ? 0 : w.l + 22 }} inert={folded >= 2}>
            <div className="ar-list" style={{ width: w.l }}>
              <div className="flex items-center gap-2 pr-2 pb-2">
                <SearchInput ref={searchRef} aria-label="找一篇" placeholder="找一篇…" value={search} onChange={(e) => setSearch(e.target.value)} className="min-w-0 flex-1" />
                {view === "trash" && !tag ? (
                  <IconButton label="清空废纸篓" danger disabled={!overview?.counts.trash} onClick={() => void emptyTrash()}>
                    <Trash2 size={17} />
                  </IconButton>
                ) : (
                  <IconButton label="新写一篇" onClick={() => void create()}>
                    <SquarePen size={18} />
                  </IconButton>
                )}
              </div>
              <div className="ar-rows pm-scroll -m-1 space-y-0 p-1 pr-2 pb-4">
                {list.map((a) => (
                  <ListRow key={a.id} a={a} trash={a.trashed} on={several ? picked.includes(a.id) : a.id === sel && !fresh} onOpen={click(a.id)} onMenu={rowMenu(a)} />
                ))}
                {q && (near.length > 0 || similar.isFetching) && (
                  <>
                    <p className="flex items-center gap-1.5 px-3 pt-3 pb-1 text-sm text-muted-foreground">
                      <Sparkles size={14} strokeWidth={1.75} /> 意思相近{similar.isFetching && near.length === 0 && <span className="animate-pulse">· 正在找…</span>}
                    </p>
                    {near.map((a) => (
                      <ListRow key={`n${a.id}`} a={a} trash={a.trashed} on={several ? picked.includes(a.id) : a.id === sel && !fresh} onOpen={click(a.id)} onMenu={rowMenu(a)} />
                    ))}
                  </>
                )}
                {empty && <p className="px-3 py-10 text-center text-sm text-muted-foreground">{q ? "没有找到" : view === "trash" && !tag ? "废纸篓是空的" : overview?.counts.all === 0 && view === "all" && !tag ? "见字如面，写下第一篇吧" : "这里还没有"}</p>}
              </div>
            </div>
            <div className={`ar-grip ${drag === "l" ? "on" : ""}`} role="separator" aria-orientation="vertical" aria-label="调宽列表" onPointerDown={startDrag("l")} />
        </div>
        <div ref={slot as React.RefObject<HTMLDivElement>} className="ar-slot" />
        {shelf && !focus && showEditor && (
          <Shelf onClose={toggleShelf} onHover={(x, y) => editor.current?.dropHint(x, y) ?? false} onDrop={(x, y, line) => editor.current?.dropLine(x, y, line) ?? false} />
        )}
        {createPortal(
          <>
          {several ? (
            <>
              {head(null)}
              <div className="ar-picked">
                <div className="grid justify-items-center gap-3">
                  <p className="font-bold">选中了 {picked.length} 篇</p>
                  <Button
                    onClick={() => {
                      const ids = rows.map((r) => r.id).filter((x) => picked.includes(x));
                      setExporting({ target: { ids }, label: `选中的 ${ids.length} 篇` });
                    }}
                  >
                    <FileOutput size={14} /> 导出…
                  </Button>
                </div>
              </div>
            </>
          ) : lockedShut ? (
            <>
              {head(article)}
              <div className="ar-locked">
                {vault?.state === "new" ? (
                  <p className="text-sm text-muted-foreground">芝麻开门还没设主密码</p>
                ) : vault ? (
                  <div className="text-center">
                    <p className="mb-1 font-bold">{article.title || "无标题"}</p>
                    <Unlock color="var(--module-articles)" status={vault} autoTouch={false} />
                  </div>
                ) : null}
              </div>
            </>
          ) : showEditor ? (
            <ArticleEditor
              key={editorKey}
              ref={editor}
              article={fresh ? null : article}
              seed={fresh?.seed}
              titles={titles}
              tags={tagNames}
              look={look}
              onBar={(bar) => setLook({ ...look, bar })}
              onCreated={(a) => {
                setSel(a.id);
                setFresh(null);
                qc.setQueryData([...ARTICLES_KEY, "one", a.id], a);
              }}
              onOpenTitle={(t) => {
                // the whole target, else before #小标题 or /小标题 (articles.rs `names`)
                const tries = [t, t.split("#")[0], t.replace(/\/[^/]*$/, "")].map((x) => x.trim().toLowerCase()).filter(Boolean);
                const hit = tries.map((k) => titles.find((x) => x.title.toLowerCase() === k)).find(Boolean);
                if (hit) void open(hit.id);
                else toast(`没有叫「${t}」的笔记`);
              }}
              onOpenTag={(t) => void go(null, t)}
              onOpenId={(id) => void open(id)}
              readOnly={!!article?.trashedAt}
              head={head(fresh ? null : article)}
            />
          ) : (
            <>
              {head(null)}
              <div className="flex-1" />
            </>
          )}
          </>,
          host,
        )}
      </div>
      {confirmDialog}
      {menu && <Menu {...menu} onClose={() => setMenu(null)} />}
      {exporting && <ExportDialog target={exporting.target} label={exporting.label} onClose={() => setExporting(null)} />}
    </div>
  );
}

