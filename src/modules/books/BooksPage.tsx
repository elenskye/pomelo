import { useEffect, useMemo, useRef, useState } from "react";
import { Plus } from "lucide-react";
import { moduleById } from "@/app/modules";
import { useConfirm } from "@/shared/feedback";
import { STATUS_ORDER, type StatusCode } from "@/shared/status";
import { Button, EmptyState, PageHeader, SearchInput, Segmented, Select } from "@/shared/ui";
import { BOOK_STATUS, useBooks, type Book } from "./api";
import { BookPanel } from "./BookPanel";
import { BookTable } from "./BookTable";

type Filter = "all" | StatusCode;
type Sort = "recent" | "title" | "rating" | "finished";
type Selection = number | "new" | null;

const SORTS: { value: Sort; label: string }[] = [
  { value: "recent", label: "最近添加" },
  { value: "title", label: "书名" },
  { value: "rating", label: "评分" },
  { value: "finished", label: "完成时间" },
];

const collator = new Intl.Collator("zh-CN");

function arrange(books: Book[], query: string, filter: Filter, sort: Sort): Book[] {
  const q = query.trim().toLowerCase();
  const xs = books.filter(
    (b) =>
      (filter === "all" || b.status === filter) &&
      (!q || [b.title, b.author, b.country, b.note].some((v) => v?.toLowerCase().includes(q))),
  );
  const byRecent = (a: Book, b: Book) => b.id - a.id;
  const cmp: Record<Sort, (a: Book, b: Book) => number> = {
    recent: byRecent,
    title: (a, b) => collator.compare(a.title, b.title),
    rating: (a, b) => (b.rating ?? 0) - (a.rating ?? 0) || byRecent(a, b),
    finished: (a, b) => (b.finishedMonth ?? "").localeCompare(a.finishedMonth ?? "") || byRecent(a, b),
  };
  return xs.sort(cmp[sort]);
}

const distinct = (xs: (string | null)[]) =>
  [...new Set(xs.filter((x): x is string => !!x))].sort((a, b) => collator.compare(a, b));

export default function BooksPage() {
  const m = moduleById("books");
  const { data: books = [], isPending } = useBooks();
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<Filter>("all");
  const [sort, setSort] = useState<Sort>("recent");
  const [selection, setSelection] = useState<Selection>(null);
  const dirty = useRef(false);
  const searchRef = useRef<HTMLInputElement>(null);
  const [confirm, confirmDialog] = useConfirm();

  const counts = useMemo(() => {
    const c: Record<StatusCode, number> = { unmarked: 0, todo: 0, doing: 0, done: 0 };
    books.forEach((b) => c[b.status]++);
    return c;
  }, [books]);
  const visible = useMemo(() => arrange(books, query, filter, sort), [books, query, filter, sort]);
  const authors = useMemo(() => distinct(books.map((b) => b.author)), [books]);
  const countries = useMemo(() => distinct(books.map((b) => b.country)), [books]);
  const selectedBook = typeof selection === "number" ? (books.find((b) => b.id === selection) ?? null) : null;

  const requestSelect = async (next: Selection) => {
    if (next === selection) return;
    if (dirty.current) {
      const ok = await confirm({ title: "放弃没有保存的修改？", confirmLabel: "放弃修改", danger: true });
      if (!ok) return;
    }
    dirty.current = false;
    setSelection(next);
  };
  const done = () => {
    dirty.current = false;
    setSelection(null);
  };

  // ⌘N new book · ⌘F search · Esc close panel (native dialogs handle their own Esc)
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (document.querySelector("dialog[open]")) return;
      if (e.metaKey && e.key === "n") {
        e.preventDefault();
        void requestSelect("new");
      } else if (e.metaKey && e.key === "f") {
        e.preventDefault();
        searchRef.current?.focus();
      } else if (e.key === "Escape" && selection !== null) {
        void requestSelect(null);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const stats = (
    <p className="pointer-events-none text-sm text-muted-foreground">
      共 <b className="text-base text-foreground">{books.length}</b>
      {(["done", "doing", "todo", "unmarked"] as const)
        .filter((s) => s !== "unmarked" || counts.unmarked > 0)
        .map((s) => (
          <span key={s}>
            {" · "}
            {BOOK_STATUS[s]} <b className="text-base text-foreground">{counts[s]}</b>
          </span>
        ))}
    </p>
  );

  return (
    <div className="relative flex h-full min-h-0 flex-col overflow-hidden">
      <PageHeader icon={m.icon} color={m.color} title={m.label} right={books.length > 0 && stats} />

      {books.length > 0 && (
        <div className="flex shrink-0 flex-wrap items-center gap-3 px-8 pb-4">
          <div className="w-60">
            <SearchInput
              ref={searchRef}
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="搜索书名、作者、国家、备注"
            />
          </div>
          <Segmented
            ariaLabel="按状态筛选"
            value={filter}
            onChange={setFilter}
            options={[
              { value: "all" as Filter, label: `全部 ${books.length}` },
              ...STATUS_ORDER.map((s) => ({ value: s as Filter, label: `${BOOK_STATUS[s]} ${counts[s]}` })),
            ]}
          />
          <div className="flex-1" />
          <Select ariaLabel="排序" value={sort} onChange={(v) => setSort(v as Sort)}>
            {SORTS.map((s) => (
              <option key={s.value} value={s.value}>
                {s.label}
              </option>
            ))}
          </Select>
          <Button variant="primary" onClick={() => void requestSelect("new")}>
            <Plus size={14} /> 新书
          </Button>
        </div>
      )}

      <div className="flex min-h-0 flex-1 flex-col overflow-y-auto px-8 pb-8">
        {isPending ? null : books.length === 0 ? (
          <EmptyState
            title="书架空空，去摸会儿鱼吧"
            hint="也可以在「设置」里从 Excel 转换来的文件导入书单"
            action={
              <Button variant="primary" onClick={() => void requestSelect("new")}>
                <Plus size={14} /> 新书
              </Button>
            }
          />
        ) : visible.length === 0 ? (
          <EmptyState title="没有找到符合条件的书" />
        ) : (
          <BookTable
            books={visible}
            selectedId={typeof selection === "number" ? selection : null}
            onSelect={(id) => void requestSelect(id)}
          />
        )}
      </div>

      {selection !== null && (selection === "new" || selectedBook) && (
        <BookPanel
          key={selection}
          book={selectedBook}
          authors={authors}
          countries={countries}
          onClose={() => void requestSelect(null)}
          onDone={done}
          onDirtyChange={(d) => {
            dirty.current = d;
          }}
        />
      )}
      {confirmDialog}
    </div>
  );
}
