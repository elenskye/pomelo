// The Markdown-lite box (lite.ts) on LiveText: 念念不忘's new note and in-place edit, the 速记小窗; any
// module wanting the same writing feel uses it (without `tags`, #words are plain text). Grows with the
// text up to 60vh (`grow`: no cap of its own, a box around it scrolls); ⌘↩ submits, Esc cancels;
// ⌘B / ⌘U / ⌘⇧H wrap the selection in ** / ++ / ==; ↩ continues a list, Tab / ⇧Tab indent it; with
// `tags`, typing `#…` lists existing tags at the caret; with `links`, typing `[[…` lists what to link to (柚子链接)
// and the right-click list starts with 「柚子链接…」 (LinkPicker: the picked words become a link, or a chip goes in).
import { forwardRef, useCallback, useMemo, useRef, useState } from "react";
import { Brackets } from "@/shared/icons";
import { liteFlavor } from "./lite";
import { LiveText, type Flavor, type LiveTextHandle, type Suggest } from "./LiveText";
import { linkInsert, typingLink, useLinkSource } from "@/shared/links/LinkParts";
import { LinkPicker, linkText } from "@/shared/links/LinkPicker";
import { selectionAnchor, type Anchor } from "@/shared/fit";
import type { LinkModule } from "@/shared/links/links";

export type { LiveTextHandle } from "./LiveText";

const WITH_TAGS = liteFlavor({ tags: true });
const PLAIN = liteFlavor({ tags: false });

/** The `#partial` tag being typed right before the caret, if any. */
function typingTag(value: string, caret: number): { start: number; query: string } | null {
  const m = /(?:^|\s)#([^\s#]*)$/u.exec(value.slice(0, caret));
  if (!m) return null;
  return { start: caret - m[1].length - 1, query: m[1] };
}

function tagSuggest(tags: string[]): Suggest {
  return {
    label: "标签",
    at: typingTag,
    // a complete tag: ↩ is a new line again
    items: (q) =>
      tags.includes(q)
        ? []
        : tags
            .filter((t) => t !== q && t.toLowerCase().includes(q.toLowerCase()))
            .sort((a, b) => Number(!a.startsWith(q)) - Number(!b.startsWith(q)))
            .slice(0, 8),
    show: (t) => `#${t}`,
    insert: (t, after) => `#${t}${after.startsWith(" ") ? "" : " "}`,
  };
}

export const LiteEditor = forwardRef<
  LiveTextHandle,
  {
    value: string;
    onChange: (v: string) => void;
    onSubmit: () => void;
    onCancel?: () => void;
    /** Existing tags: #tags are drawn as tags and suggested while typed. */
    tags?: string[];
    placeholder?: string;
    label?: string;
    autoFocus?: boolean;
    minRows?: number;
    grow?: boolean;
    /** 柚子链接: `[[` lists articles, notes and works (`except`: the note being edited). */
    links?: { except: [LinkModule, number] | null };
  }
>(function LiteEditor(props, handle) {
  const src = useLinkSource(props.links?.except ?? null);
  const on = !!props.links;
  const suggest = useMemo((): Suggest | undefined => {
    const tags = props.tags ? tagSuggest(props.tags) : undefined;
    if (!on) return tags;
    return {
      label: tags ? "链接和标签" : "链接",
      at: (v, c) => {
        const l = typingLink(v, c);
        if (l) return { start: l.start, query: `\u0001${l.query}` };
        return tags ? tags.at(v, c) : null;
      },
      items: (q) => (q.startsWith("\u0001") ? src.items(q.slice(1)) : (tags?.items(q) ?? [])),
      show: (item) => (src.isLink(item) ? src.show(item) : `#${item}`),
      insert: (item, after) => {
        const t = src.isLink(item) ? src.thing(item) : null;
        return t ? linkInsert(t, after) : (tags?.insert(item, after) ?? item);
      },
    };
  }, [props.tags, on, src.version]); // eslint-disable-line react-hooks/exhaustive-deps
  const inner = useRef<LiveTextHandle | null>(null);
  const setRef = useCallback(
    (h: LiveTextHandle | null) => {
      inner.current = h;
      if (typeof handle === "function") handle(h);
      else if (handle) handle.current = h;
    },
    [handle],
  );
  const [linking, setLinking] = useState<{ at: Anchor; sel: [number, number] } | null>(null);
  const tagged = !!props.tags;
  const flavor = useMemo((): Flavor => {
    const base = tagged ? WITH_TAGS : PLAIN;
    if (!on) return base;
    return { ...base, formats: [{ label: "柚子链接…", lead: true, icon: <Brackets size={16} />, act: (sel) => setLinking({ at: selectionAnchor(), sel }) }, "-", ...(base.formats ?? [])] };
  }, [tagged, on]);
  return (
    <>
    <LiveText
      ref={setRef}
      value={props.value}
      onChange={props.onChange}
      onSubmit={props.onSubmit}
      onCancel={props.onCancel}
      flavor={flavor}
      suggest={suggest}
      label={props.label ?? props.placeholder ?? "笔记"}
      placeholder={props.placeholder}
      autoFocus={props.autoFocus}
      style={{ minHeight: `${(props.minRows ?? 3) * 1.625}em` }}
      className={props.grow ? "" : "max-h-[60vh] overflow-y-auto"}
    />
    {linking && (
      <LinkPicker
        at={linking.at}
        except={props.links?.except ?? null}
        onClose={() => setLinking(null)}
        onPick={(t) => {
          const [from, to] = linking.sel;
          const text = linkText(t.target, inner.current?.text().slice(from, to) ?? "");
          inner.current?.edit(from, to, text, [from + text.length, from + text.length]);
          setLinking(null);
          inner.current?.select(from + text.length); // back in the box, the caret after the link
        }}
      />
    )}
    </>
  );
});
