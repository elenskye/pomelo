// ASK (Rust youzi/ask.rs): a reply's questions to me. While it waits: a card under the text, one or a few questions, each
// with its options (单选, 多选, 判断) and 其他, answered together. Once answered, a quiet line above the reply says what was
// asked and what I said. The composer can't send meanwhile (the conversation is busy): only this card answers, or 停止.
// A reply that ended while waiting (the app quit): the card stays, and the answer goes on as a new message.
import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { errorText, useToast } from "@/shared/feedback";
import { Button, TextInput } from "@/shared/ui";
import { youziApi, STATUS_KEY, type Ask, type AskAnswer } from "./api";

const said = (a: AskAnswer) => [...a.picked, ...(a.other.trim() ? [a.other.trim()] : [])].join("、");

export function AskCard(props: { ask: Ask; onFollowUp?: (text: string) => void }) {
  const { questions } = props.ask;
  const qc = useQueryClient();
  const toast = useToast();
  const [answers, setAnswers] = useState<AskAnswer[]>(() => questions.map(() => ({ picked: [], other: "" })));
  const [busy, setBusy] = useState(false);
  const set = (i: number, change: Partial<AskAnswer>) => setAnswers(answers.map((a, k) => (k === i ? { ...a, ...change } : a)));
  const pick = (i: number, option: string, multi: boolean) => {
    const was = answers[i].picked;
    set(i, { picked: multi ? (was.includes(option) ? was.filter((o) => o !== option) : [...was, option]) : [option] });
  };
  const ready = answers.every((a) => a.picked.length > 0 || a.other.trim());
  const send = async () => {
    setBusy(true);
    try {
      const r = await youziApi.askAnswer(props.ask.id, answers);
      void qc.invalidateQueries({ queryKey: ["youzi", "messages"] });
      void qc.invalidateQueries({ queryKey: STATUS_KEY });
      if (r.followUp) props.onFollowUp?.(r.followUp);
    } catch (e) {
      toast(errorText(e), "error");
      setBusy(false);
    }
  };
  return (
    <div data-find-skip data-ask={props.ask.id} className="mt-2 max-w-md space-y-3 rounded-card border bg-card p-3 text-sm">
      {questions.map((q, i) => (
        <fieldset key={i} disabled={busy} className="space-y-1">
          <legend className="pb-1 font-bold break-words">{q.question}</legend>
          {q.options.map((o) => (
            <label key={o} className="flex cursor-default items-start gap-2 rounded-control px-1 py-0.5 hover:bg-muted/60">
              <input type={q.kind === "multi" ? "checkbox" : "radio"} name={`ask-${props.ask.id}-${i}`} checked={answers[i].picked.includes(o)} onChange={() => pick(i, o, q.kind === "multi")} className="mt-1 size-4 shrink-0 accent-[var(--primary)]" />
              <span className="min-w-0 break-words">{o}</span>
            </label>
          ))}
          <TextInput aria-label="其他" placeholder="其他" maxLength={1000} value={answers[i].other} onChange={(e) => set(i, { other: e.target.value, ...(q.kind === "multi" ? {} : { picked: [] }) })} className="mt-1" />
        </fieldset>
      ))}
      <div className="flex justify-end">
        <Button variant="primary" className="py-1" disabled={busy || !ready} onClick={() => void send()}>
          回答
        </Button>
      </div>
    </div>
  );
}

/** What it asked and I said, above the reply; a card closed unanswered says so. */
export function AskedLines(props: { asks: Ask[] }) {
  const done = props.asks.filter((a) => a.state !== "waiting");
  if (!done.length) return null;
  return (
    <div data-find-skip className="mb-2 space-y-0.5 text-xs text-muted-foreground">
      {done.flatMap((a) =>
        a.questions.map((q, i) => (
          <p key={`${a.id}-${i}`} className="break-words">
            问了你：{q.question} → <span className="text-foreground">{a.answers?.[i] ? said(a.answers[i]) : "没回答"}</span>
          </p>
        )),
      )}
    </div>
  );
}
