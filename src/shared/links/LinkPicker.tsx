// 柚子链接…: the finder a right-click opens in 见字如面 and 念念不忘 — an article, a note, a book, a film or a game; the
// picked words become a Pomelo link to it (`[[目标|字]]`), or with nothing picked a chip goes in at the caret (`[[目标]]`).
import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useHiddenModules } from "@/app/hidden";
import { X } from "@/shared/icons";
import { IconButton } from "@/shared/ui";
import { LinkMark } from "./LinkParts";
import { dayText, linksApi, type Thing } from "./links";

export type At = { left: number; top: number };

/** Where a box goes under the selection (kept inside the window). */
export function underSelection(width: number, height = 260): At {
  const sel = window.getSelection();
  const r = sel && sel.rangeCount ? sel.getRangeAt(0).getBoundingClientRect() : null;
  const left = Math.max(12, Math.min((r?.left ?? 200) - 8, window.innerWidth - width - 12));
  const top = Math.max(12, Math.min((r?.bottom ?? 200) + 10, window.innerHeight - height));
  return { left, top };
}

/** What goes into the text for a picked thing: the words as a link to it, or (no words, or words a link can't hold) a chip. */
export const linkText = (target: string, words: string) => (words.trim() && !/[\n\]|[]/.test(words) ? `[[${target}|${words}]]` : `[[${target}]]`);

export function LinkPicker(props: { at: At; except: [string, number] | null; onPick: (t: Thing) => void; onClose: () => void }) {
  const [q, setQ] = useState("");
  const [items, setItems] = useState<Thing[]>([]);
  const [active, setActive] = useState(0);
  const [hidden] = useHiddenModules();
  const box = useRef<HTMLDivElement>(null);
  useEffect(() => {
    let live = true;
    const t = setTimeout(() => {
      void linksApi
        .suggest(q, hidden, props.except)
        .then((r) => live && (setItems(r), setActive(0)))
        .catch(() => {});
    }, 120);
    return () => {
      live = false;
      clearTimeout(t);
    };
  }, [q, hidden.join(",")]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => {
    const away = (e: PointerEvent) => !box.current?.contains(e.target as Node) && props.onClose();
    window.addEventListener("pointerdown", away, true);
    return () => window.removeEventListener("pointerdown", away, true);
  });
  return createPortal(
    <div ref={box} role="dialog" aria-label="柚子链接" className="pm-linkpick pm-menu animate-pop-in" style={{ left: props.at.left, top: props.at.top }}>
      <div className="flex items-center gap-1 px-1 pb-1">
        <input
          autoFocus
          value={q}
          onChange={(e) => setQ(e.target.value)}
          placeholder="找一篇文章、一条笔记、书影游…"
          aria-label="找要链接的"
          className="h-8 min-w-0 flex-1 rounded-control bg-transparent px-2 text-sm outline-none"
          onKeyDown={(e) => {
            if (e.nativeEvent.isComposing || e.keyCode === 229) return;
            if (e.key === "ArrowDown" || e.key === "ArrowUp") {
              e.preventDefault();
              setActive((a) => (items.length ? (a + (e.key === "ArrowDown" ? 1 : items.length - 1)) % items.length : 0));
            } else if (e.key === "Enter" && items[active]) {
              e.preventDefault();
              props.onPick(items[active]);
            } else if (e.key === "Escape") {
              e.preventDefault();
              e.stopPropagation();
              props.onClose();
            }
          }}
        />
        <IconButton label="关闭" onClick={props.onClose}>
          <X size={14} />
        </IconButton>
      </div>
      <div role="listbox" aria-label="柚子链接">
        {items.map((t, i) => (
          <button key={`${t.module}${t.id}`} type="button" role="option" aria-selected={i === active} onMouseEnter={() => setActive(i)} onClick={() => props.onPick(t)} className="pm-menu-item pm-linkpick-row">
            <span className="pm-sug">
              <LinkMark module={t.module} size={24} />
              <span className="pm-sug-t">
                <span className="pm-sug-n">{t.title || "无标题"}</span>
                {(t.meta || t.date) && <span className="pm-sug-m">{t.module === "articles" || t.module === "notes" ? dayText(t.date) : t.meta}</span>}
              </span>
            </span>
          </button>
        ))}
        {items.length === 0 && <p className="px-3 py-2 text-sm text-muted-foreground">没有找到</p>}
      </div>
    </div>,
    document.body,
  );
}
