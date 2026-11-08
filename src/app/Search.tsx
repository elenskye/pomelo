// 找东西 (全局搜索): ⌘K (or the top bar's magnifier) searches every module that isn't hidden (芝麻开门 is
// never searched — Rust doesn't look there). Words all have to match (search/mod.rs). Results are grouped
// by module; below them 意思相近 (v0.4: by meaning, a moment later, without what words found). ↑↓ move,
// ↩ opens (navigate with the hit; the page opens the record), Esc closes.
// Looks like a receipt printer (visual-atelier's frame prototype, board Search): the slot drops in, a receipt
// prints out of it, the results print line by line as you type; picking one tears the receipt off and takes you
// there. Motion: ds.css (receipt-print).
import { useEffect, useMemo, useRef, useState } from "react";
import { useTabs } from "./tabs";
import { useQuery, keepPreviousData } from "@tanstack/react-query";
import { errorText } from "@/shared/feedback";
import { reducedMotion } from "@/shared/hand";
import { HandIcon } from "@/shared/handParts";
import { ModuleIcon } from "./ModuleIcon";
import { money, type Currency } from "@/modules/finance/money";
import { slashDate } from "@/shared/dates";
import { searchAll, searchSimilar, type Hit } from "@/shared/open";
import { useAllModules } from "@/modules/custom/api";
import { useHiddenModules } from "./hidden";

const md = (d: string) => `${Number(d.slice(5, 7))}/${Number(d.slice(8))}`;

/** The muted text after a title. */
function detail(h: Hit): string | null {
  if (h.kind === "entry" && h.day && h.amount !== null && h.currency)
    return [md(h.day), money(h.amount, h.currency as Currency, h.flow === "in"), h.detail].filter(Boolean).join(" · ");
  if (h.kind === "note" && h.day) return slashDate(h.day);
  if (h.kind === "task" && h.day) return `${h.detail} · ${slashDate(h.day)}`;
  return h.detail;
}

/** The receipt's torn bottom edge. */
const TORN = (() => {
  const n = 22, zz: string[] = [];
  for (let i = 0; i <= n * 2; i++) zz.push(`${((i / (n * 2)) * 100).toFixed(2)}% ${i % 2 ? "calc(100% - 10px)" : "100%"}`);
  return `polygon(0 0,100% 0,${zz.reverse().join(",")})`;
})();

export function SearchBox(props: { onClose: () => void; onAsk?: ((text: string) => void) | null }) {
  const ref = useRef<HTMLDialogElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const tabs = useTabs();
  const [hidden] = useHiddenModules();
  const modules = useAllModules();
  const [text, setText] = useState("");
  const [q, setQ] = useState("");
  const [active, setActive] = useState(0);
  const [closing, setClosing] = useState(false);
  const paper = useRef<HTMLDivElement>(null);
  const close = () => {
    if (closing) return;
    if (reducedMotion()) return props.onClose();
    setClosing(true);
    setTimeout(props.onClose, 300);
  };
  useEffect(() => {
    const d = ref.current;
    if (d && !d.open) d.showModal();
  }, []);
  useEffect(() => {
    const t = setTimeout(() => setQ(text.trim()), 120);
    return () => clearTimeout(t);
  }, [text]);
  const { data = [], isFetched } = useQuery({ queryKey: ["search", q], queryFn: () => searchAll(q), enabled: q !== "", placeholderData: keepPreviousData });
  const similar = useQuery({ queryKey: ["search", "similar", q], queryFn: () => searchSimilar(q), enabled: q !== "", retry: false, staleTime: 30_000 });
  // grouped in the sidebar's module order (the list below and the arrow keys go the same way)
  const hits = useMemo(() => {
    const rank = (h: Hit) => modules.findIndex((m) => m.id === h.module);
    return q ? data.filter((h) => !hidden.includes(h.module) && rank(h) >= 0).sort((a, b) => rank(a) - rank(b)) : [];
  }, [data, hidden, q, modules]);
  const near = useMemo(
    () => (q && similar.data ? similar.data.filter((h) => !hidden.includes(h.module) && !hits.some((x) => x.module === h.module && x.kind === h.kind && x.id === h.id)) : []),
    [similar.data, hidden, hits, q],
  );
  const all = useMemo(() => [...hits, ...near], [hits, near]);
  useEffect(() => setActive(0), [hits]);
  useEffect(() => {
    list.current?.querySelector(`[data-index="${active}"]`)?.scrollIntoView({ block: "nearest" });
  }, [active]);

  // the receipt tears off, then the page opens
  const open = (h: Hit) => {
    const go = () => {
      props.onClose();
      tabs.open(`/${h.module}`, { open: h });
    };
    if (reducedMotion() || !paper.current) return go();
    paper.current.classList.add("is-tear");
    setTimeout(go, 340);
  };
  const onKey = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (all.length) setActive((i) => (i + (e.key === "ArrowDown" ? 1 : -1) + all.length) % all.length);
    } else if (e.key === "Enter" && !e.nativeEvent.isComposing) {
      e.preventDefault();
      if (all[active]) open(all[active]);
    }
  };

  let i = -1;
  const row = (h: Hit, near: boolean) => {
    i += 1;
    const index = i;
    const d = detail(h);
    const m = near ? modules.find((x) => x.id === h.module) : undefined;
    return (
      <div
        key={`${near ? "near-" + h.module : ""}${h.kind}${h.id}`}
        role="option"
        aria-selected={index === active}
        data-index={index}
        onMouseMove={() => setActive(index)}
        onClick={() => open(h)}
        className={`srow pm-print-row ${index === active ? "on" : ""}`}
        style={{ "--pm-i": index } as React.CSSProperties}
      >
        {m && <ModuleIcon def={m} size={18} />}
        <span className="rt">{h.title}</span>
        {d && <span className="rd">{d}</span>}
      </div>
    );
  };
  const n = hits.length;
  return (
    <dialog
      ref={ref}
      aria-label="找东西"
      onCancel={(e) => {
        e.preventDefault();
        close();
      }}
      onClick={(e) => e.target === ref.current && close()}
      className="searchdlg"
    >
      <div className={`pm-printer ${closing ? "is-closing" : "is-open"}`} onClick={(e) => e.target === e.currentTarget && close()}>
        <div className="pslot pm-print-slot" aria-hidden>
          <div className="ps-body" />
          <div className="ps-mouth" />
        </div>
        <div className="rcwrap">
          <div ref={paper} className="rcpt pm-print-paper">
            <div className="rpaper" style={{ clipPath: TORN }} />
            <div className="rin">
              <div className="rhead">— 柚儿园 · 找东西 —</div>
              <div className="sfield">
                <HandIcon name="search" size={20} seed={902} className="text-[var(--color-ink-soft)]" />
                <input autoFocus aria-label="找东西" placeholder="找点什么…" value={text} onChange={(e) => setText(e.target.value)} onKeyDown={onKey} />
                {q && n > 0 && <span className="scount">{n} 条</span>}
                <span className="pm-rule sline" data-hand="rule" data-seed="901" data-sw="1.8" />
              </div>
              {q !== "" && (
                <div ref={list} role="listbox" aria-label="找到的" className="sres">
                  {all.length === 0 && !similar.isFetching
                    ? isFetched && (
                        <div className="snone">
                          没找到「{q}」
                          {props.onAsk && (
                            <button
                              type="button"
                              className="pm-press"
                              onClick={() => {
                                props.onAsk!(q);
                                props.onClose();
                              }}
                            >
                              <HandIcon name="sparkle" size={13} seed={903} />
                              问问柚子
                            </button>
                          )}
                        </div>
                      )
                    : modules
                        .filter((m) => hits.some((h) => h.module === m.id))
                        .map((m) => (
                          <div key={m.id}>
                            <div className="sgh">
                              <ModuleIcon def={m} size={18} />
                              {m.label}
                            </div>
                            {hits.filter((h) => h.module === m.id).map((h) => row(h, false))}
                          </div>
                        ))}
                  {(near.length > 0 || similar.isFetching || similar.isError) && (
                    <div>
                      <div className="snear">
                        <i />
                        <span>
                          意思相近
                          {similar.isFetching && near.length === 0 && " · 正在找…"}
                          {similar.isError && ` · ${errorText(similar.error)}`}
                        </span>
                        <i />
                      </div>
                      {near.map((h) => row(h, true))}
                    </div>
                  )}
                </div>
              )}
              <div className="rfoot">谢谢光临 · 柚子</div>
            </div>
          </div>
        </div>
      </div>
    </dialog>
  );
}
