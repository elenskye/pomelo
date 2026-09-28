import { useEffect, useMemo, useState, type FormEvent } from "react";
import { Trash2, X } from "lucide-react";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { currentMonth, MonthInput } from "@/shared/MonthInput";
import { StarRating } from "@/shared/StarRating";
import { STATUS_ORDER, type StatusCode } from "@/shared/status";
import { Button, Field, Segmented, TextArea, TextInput } from "@/shared/ui";
import { BOOK_STATUS, useBookMutations, type Book, type BookPatch } from "./api";

type Draft = {
  title: string;
  author: string;
  country: string;
  status: StatusCode;
  rating: number | null;
  finishedMonth: string | null;
  note: string;
};

const toDraft = (b: Book | null): Draft => ({
  title: b?.title ?? "",
  author: b?.author ?? "",
  country: b?.country ?? "",
  status: b?.status ?? "unmarked",
  rating: b?.rating ?? null,
  finishedMonth: b?.finishedMonth ?? null,
  note: b?.note ?? "",
});

// Full form → every key explicit, so Rust won't auto-fill the month behind the user's back.
const toPatch = (d: Draft): BookPatch => ({
  title: d.title,
  author: d.author.trim() || null,
  country: d.country.trim() || null,
  status: d.status,
  rating: d.rating,
  finishedMonth: d.finishedMonth,
  note: d.note.trim() || null,
});

/** Right-side overlay editor. `book === null` = create mode. */
export function BookPanel(props: {
  book: Book | null;
  authors: string[];
  countries: string[];
  onClose: () => void; // user-initiated (page checks unsaved changes)
  onDone: () => void; // after save/delete
  onDirtyChange: (dirty: boolean) => void;
}) {
  const { book } = props;
  const isNew = book === null;
  const initial = useMemo(() => toDraft(book), [book]);
  const [d, setD] = useState(initial);
  const [error, setError] = useState<string | null>(null);
  const { create, update, remove } = useBookMutations();
  const toast = useToast();
  const [confirm, confirmDialog] = useConfirm();

  const dirty = JSON.stringify(d) !== JSON.stringify(initial);
  useEffect(() => {
    props.onDirtyChange(dirty);
  }, [dirty]); // eslint-disable-line react-hooks/exhaustive-deps

  const set = <K extends keyof Draft>(k: K, v: Draft[K]) => setD((x) => ({ ...x, [k]: v }));
  const setStatus = (status: StatusCode) =>
    setD((x) => ({
      ...x,
      status,
      // mirror the Rust rule visibly: becoming 已读 with no month → this month (user can change it)
      finishedMonth: status === "done" && x.status !== "done" && !x.finishedMonth ? currentMonth() : x.finishedMonth,
    }));

  const save = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    try {
      if (isNew) await create.mutateAsync(toPatch(d));
      else if (dirty) await update.mutateAsync({ id: book.id, patch: toPatch(d) });
      toast(isNew ? `已添加《${d.title.trim()}》` : "已保存");
      props.onDone();
    } catch (err) {
      setError(errorText(err));
    }
  };

  const del = async () => {
    if (!book) return;
    const ok = await confirm({
      title: `删除《${book.title}》？`,
      message: "删除后无法撤销，只能从备份里找回。",
      confirmLabel: "删除",
      danger: true,
    });
    if (!ok) return;
    try {
      await remove.mutateAsync(book.id);
      toast(`已删除《${book.title}》`);
      props.onDone();
    } catch (err) {
      setError(errorText(err));
    }
  };

  return (
    <aside className="absolute inset-y-0 right-0 z-10 flex w-[400px] animate-slide-in flex-col border-l bg-card shadow-2xl">
      <form onSubmit={save} className="flex min-h-0 flex-1 flex-col">
        <header data-tauri-drag-region className="flex h-16 shrink-0 items-end justify-between px-6 pb-3">
          <h2 className="pointer-events-none text-base font-bold">{isNew ? "新书" : "编辑"}</h2>
          <button
            type="button"
            aria-label="关闭"
            onClick={props.onClose}
            className="rounded-control p-1 text-muted-foreground hover:bg-muted"
          >
            <X size={16} />
          </button>
        </header>

        <div className="flex-1 space-y-4 overflow-y-auto px-6 py-2">
          <Field label="书名">
            <TextInput autoFocus value={d.title} onChange={(e) => set("title", e.target.value)} placeholder="必填" />
          </Field>
          <Field label="作者">
            <TextInput list="book-authors" value={d.author} onChange={(e) => set("author", e.target.value)} placeholder="多位作者用「 / 」隔开" />
          </Field>
          <Field label="国家">
            <TextInput list="book-countries" value={d.country} onChange={(e) => set("country", e.target.value)} />
          </Field>
          <Field label="状态">
            <Segmented
              ariaLabel="状态"
              value={d.status}
              options={STATUS_ORDER.map((s) => ({ value: s, label: BOOK_STATUS[s] }))}
              onChange={setStatus}
            />
          </Field>
          <Field label="尝尝咸淡">
            <StarRating size={20} value={d.rating} onChange={(v) => set("rating", v)} />
          </Field>
          <Field label="完成时间">
            <MonthInput value={d.finishedMonth} onChange={(v) => set("finishedMonth", v)} />
          </Field>
          <Field label="备注">
            <TextArea rows={5} value={d.note} onChange={(e) => set("note", e.target.value)} />
          </Field>
          {error && <p className="text-sm text-destructive">{error}</p>}
          {book && (
            <p className="pt-2 text-xs text-muted-foreground">
              添加于 {new Date(book.createdAt).toLocaleDateString("zh-CN")}
            </p>
          )}
        </div>

        <footer className="flex shrink-0 items-center gap-2 border-t px-6 py-3">
          {book && (
            <Button variant="ghost-danger" onClick={del}>
              <Trash2 size={14} /> 删除
            </Button>
          )}
          <div className="flex-1" />
          <Button onClick={props.onClose}>取消</Button>
          <Button type="submit" variant="primary">
            保存
          </Button>
        </footer>
      </form>

      <datalist id="book-authors">
        {props.authors.map((a) => (
          <option key={a} value={a} />
        ))}
      </datalist>
      <datalist id="book-countries">
        {props.countries.map((c) => (
          <option key={c} value={c} />
        ))}
      </datalist>
      {confirmDialog}
    </aside>
  );
}
