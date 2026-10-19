// 年度总结 on 首页 (rules in Rust picky/yearly.rs `card`): in the new year's first week (1 月 1～7 日),
// last year's letter above 今日简报 — its 关键词 line and opening; 看全文 opens it in a dialog, which
// can open the conversation in 小拾. Hidden when 小拾 is hidden.
import { useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router";
import { Bird, X } from "lucide-react";
import { MarkdownView } from "@/modules/picky/MarkdownView";
import { useYearCard, type YearLetter } from "@/modules/picky/api";
import { InlineMd } from "@/shared/InlineMd";
import { Button, IconButton } from "@/shared/ui";

/** The 关键词 line and the first paragraph (notes in （） from Rust left out). */
function opening(content: string): [string | null, string | null] {
  const lines = content
    .split("\n")
    .map((l) => l.trim())
    .filter((l) => l && !/^（.*）$/.test(l));
  const keywords = lines.find((l) => l.includes("关键词")) ?? null;
  const first = lines.find((l) => l !== keywords && !/^(#|>|[-*+] |\d+\. |\|)/.test(l)) ?? null;
  return [keywords, first];
}

export function YearCard() {
  const { data: letter } = useYearCard();
  const [reading, setReading] = useState(false);
  if (!letter) return null;
  const [keywords, first] = opening(letter.content);
  return (
    <section className="rounded-card border bg-card px-4 py-3">
      <div className="flex items-center gap-1.5">
        <button type="button" onClick={() => setReading(true)} className="flex items-center gap-1.5 rounded-control text-sm font-bold outline-none focus-visible:ring-2 focus-visible:ring-ring">
          <Bird size={15} strokeWidth={1.75} style={{ color: "var(--module-picky)" }} />
          {letter.year} 年度总结
        </button>
        <div className="flex-1" />
        <button
          type="button"
          onClick={() => setReading(true)}
          className="rounded-control px-1.5 py-0.5 text-xs text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
        >
          看全文
        </button>
      </div>
      {keywords && (
        <p className="mt-2 text-sm">
          <InlineMd text={keywords.replace(/^#+\s*/, "")} />
        </p>
      )}
      {first && (
        <p className="mt-1 line-clamp-2 text-sm leading-relaxed text-muted-foreground">
          <InlineMd text={first} />
        </p>
      )}
      {reading && <LetterDialog letter={letter} onClose={() => setReading(false)} />}
    </section>
  );
}

function LetterDialog(props: { letter: YearLetter; onClose: () => void }) {
  const ref = useRef<HTMLDialogElement>(null);
  const navigate = useNavigate();
  useEffect(() => {
    const d = ref.current;
    if (d && !d.open) d.showModal();
  }, []);
  const title = `${props.letter.year} 年度总结`;
  const openChat = () => {
    props.onClose();
    void navigate("/picky", {
      state: { open: { module: "picky", kind: "session", id: props.letter.sessionId, title, detail: null, parent: null, day: null, amount: null, currency: null, flow: null } },
    });
  };
  return (
    <dialog
      ref={ref}
      aria-label={title}
      onCancel={(e) => {
        e.preventDefault();
        props.onClose();
      }}
      className="m-auto flex h-[82vh] max-h-[900px] w-[720px] max-w-[92vw] flex-col rounded-card border bg-popover p-0 text-foreground shadow-xl backdrop:bg-black/30"
    >
      <div className="flex items-center gap-4 border-b px-5 py-3">
        <p className="text-base font-bold">{title}</p>
        <div className="flex-1" />
        <IconButton label="关闭" onClick={props.onClose}>
          <X size={16} />
        </IconButton>
      </div>
      <div className="-m-1 min-h-0 flex-1 overflow-y-auto p-1">
        <div className="px-8 py-6">
          <MarkdownView body source={props.letter.content} />
        </div>
      </div>
      <div className="flex items-center border-t px-5 py-3">
        <div className="flex-1" />
        <Button onClick={openChat}>在小拾里打开</Button>
      </div>
    </dialog>
  );
}
