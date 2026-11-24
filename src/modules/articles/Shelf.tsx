// 书架: while writing, a narrow column of 念念不忘 at the right of the text (282 wide): find a note (words, a tag), and
// drag one into the text — it goes in as `![[念念不忘/…]]` (the note set in as a small card; links.rs). The drag is
// pointer events (not the browser's own): the note under the pointer leans a little, its place in the column fades, a
// line in the text shows where it lands; Esc puts it back.
import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Dropdown } from "@/shared/Dropdown";
import { GripVertical, X } from "@/shared/icons";
import { errorText, useToast } from "@/shared/feedback";
import { IconButton, SearchInput } from "@/shared/ui";
import { useNotes, useOverview, type Note } from "@/modules/notes/api";
import { NoteBody } from "@/modules/notes/NoteBody";
import { linksApi } from "@/shared/links/links";
import { LinkLookup, LinkMark } from "@/shared/links/LinkParts";
import { pad } from "@/shared/dates";
import { listDate } from "./api";

function when(iso: string) {
  const d = new Date(iso);
  return `${listDate(iso)} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

type Drag = { note: Note; x: number; y: number; w: number };

export function Shelf(props: {
  onClose: () => void;
  /** Over the text: where it would go (false: not over it). */
  onHover: (x: number, y: number) => boolean;
  /** Let go: put the note's set-in line there (false: not over the text). */
  onDrop: (x: number, y: number, line: string) => boolean;
}) {
  const [q, setQ] = useState("");
  const [query, setQuery] = useState("");
  const [tag, setTag] = useState<string | null>(null);
  useEffect(() => {
    const t = setTimeout(() => setQuery(q.trim()), 200);
    return () => clearTimeout(t);
  }, [q]);
  const notes = useNotes({ q: query || undefined, tag: tag ?? undefined }).data ?? [];
  const tags = useOverview().data?.tags ?? [];
  const toast = useToast();
  const [drag, setDrag] = useState<Drag | null>(null);
  const targetOf = useRef(new Map<number, string>());

  const start = (n: Note) => (e: React.PointerEvent<HTMLElement>) => {
    if (e.button !== 0) return;
    const card = e.currentTarget.getBoundingClientRect();
    const sx = e.clientX;
    const sy = e.clientY;
    let d: Drag | null = null;
    // what the note is written as in a link (asked now, there by the time it's dropped)
    if (!targetOf.current.has(n.id)) void linksApi.thing("notes", n.id).then((t) => t && targetOf.current.set(n.id, t.target));
    const move = (ev: PointerEvent) => {
      if (!d && Math.hypot(ev.clientX - sx, ev.clientY - sy) < 6) return;
      if (!d) window.getSelection()?.removeAllRanges();
      d = { note: n, x: ev.clientX - (sx - card.left), y: ev.clientY - (sy - card.top), w: card.width };
      setDrag(d);
      props.onHover(ev.clientX, ev.clientY);
    };
    const stop = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", cancel);
      window.removeEventListener("keydown", key, true);
      setDrag(null);
    };
    const up = (ev: PointerEvent) => {
      stop();
      if (!d) return;
      const target = targetOf.current.get(n.id);
      if (!target) {
        props.onHover(-1, -1);
        return toast("这条笔记还没准备好，再拖一次", "error");
      }
      try {
        props.onDrop(ev.clientX, ev.clientY, `![[${target}]]`);
      } catch (err) {
        toast(errorText(err), "error");
      }
    };
    const cancel = () => {
      stop();
      props.onHover(-1, -1);
    };
    const key = (ev: KeyboardEvent) => {
      if (ev.key !== "Escape") return;
      ev.preventDefault();
      ev.stopPropagation();
      cancel();
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", cancel);
    window.addEventListener("keydown", key, true);
  };

  return (
    <aside className="ar-shelf" aria-label="念念不忘">
      <div className="ar-shelf-head">
        <LinkMark module="notes" size={24} />
        <b>念念不忘</b>
        <span className="flex-1" />
        <IconButton label="收起" onClick={props.onClose}>
          <X size={16} />
        </IconButton>
      </div>
      <SearchInput value={q} onChange={(e) => setQ(e.target.value)} placeholder="找一条…" />
      <Dropdown<string | null>
        ariaLabel="按标签"
        value={tag}
        onChange={setTag}
        options={[{ value: null, label: "全部笔记" }, ...tags.map((t) => ({ value: t.tag as string | null, label: <span className="pm-hashtag">#{t.tag}</span> }))]}
        menuMaxHeight={300}
      />
      <LinkLookup texts={notes.slice(0, 60).map((n) => n.content)}>
        <div className="ar-shelf-list pm-scroll -m-1 p-1">
          {notes.slice(0, 60).map((n) => (
            <div key={n.id} className={`ar-shelf-note ${drag?.note.id === n.id ? "faded" : ""}`} onPointerDown={start(n)} onDragStart={(e) => e.preventDefault()}>
              <span className="ar-shelf-when">{when(n.createdAt)}</span>
              <div className="ar-shelf-text">
                <NoteBody content={n.content} />
              </div>
            </div>
          ))}
          {notes.length === 0 && <p className="px-2 py-6 text-center text-sm text-muted-foreground">没有找到</p>}
        </div>
      </LinkLookup>
      {drag &&
        createPortal(
          <div className="ar-shelf-note ar-shelf-drag" style={{ left: drag.x, top: drag.y, width: drag.w }}>
            <span className="ar-shelf-when">
              {when(drag.note.createdAt)}
              <GripVertical size={14} className="ml-auto" />
            </span>
            <div className="ar-shelf-text">
              <NoteBody content={drag.note.content} />
            </div>
          </div>,
          document.body,
        )}
    </aside>
  );
}
