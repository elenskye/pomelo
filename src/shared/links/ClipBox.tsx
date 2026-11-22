// 念念不忘's box, floating where a note is jotted from somewhere else (柚子链接): 「记下」 saves the note, the caret starts
// at the end to go on writing and add #tags.
// - 见字如面's 记到念念不忘: under the words picked in the text, filled with them and 「来自 [[这篇]]」 (the note links back).
// - a book's, film's or game's panel, 「记一条」: starts with a link to it (the note shows in its 柚子链接).
import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { LiteEditor, type LiveTextHandle } from "@/shared/editor/LiteEditor";
import { errorText, useToast } from "@/shared/feedback";
import { useDragFit, type Anchor } from "@/shared/fit";
import { Button } from "@/shared/ui";
import { useNoteMutations, useOverview } from "@/modules/notes/api";
import { LinkMark } from "./LinkParts";
import "./links.css";

export function ClipBox(props: { at: Anchor; text: string; label?: string; onClose: () => void }) {
  const [draft, setDraft] = useState(props.text);
  const m = useNoteMutations();
  const toast = useToast();
  const tags = (useOverview().data?.tags ?? []).map((t) => t.tag);
  const editor = useRef<LiveTextHandle>(null);
  const box = useRef<HTMLDivElement>(null);
  const { style: place, grab } = useDragFit(box, props.at);
  useEffect(() => {
    requestAnimationFrame(() => {
      editor.current?.focus();
      editor.current?.select(props.text.length);
    });
  }, []); // eslint-disable-line react-hooks/exhaustive-deps
  const submit = () => {
    if (!draft.trim() || m.create.isPending) return;
    m.create.mutate(
      { content: draft },
      {
        onSuccess: () => {
          toast("记到念念不忘了");
          props.onClose();
        },
        onError: (e) => toast(errorText(e), "error"),
      },
    );
  };
  return createPortal(
    <div ref={box} role="dialog" aria-label={props.label ?? "记到念念不忘"} className="pm-clip pm-float animate-pop-in" style={place} data-hand="card" data-seed={1549} data-amp="0.8" data-drag="" onMouseDown={grab}>
      <span className="pm-clip-slap" aria-hidden>
        <LinkMark module="notes" size={40} />
      </span>
      <LiteEditor ref={editor} value={draft} onChange={setDraft} onSubmit={submit} onCancel={props.onClose} tags={tags} links={{ except: null }} label="记到念念不忘" minRows={2} />
      <div className="mt-2 flex items-center justify-end gap-2">
        <Button variant="ghost" onClick={props.onClose}>
          取消
        </Button>
        <Button variant="primary" disabled={!draft.trim() || m.create.isPending} onClick={submit}>
          记下
        </Button>
      </div>
    </div>,
    document.body,
  );
}
