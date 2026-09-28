import { StarRating } from "@/shared/StarRating";
import { StatusSelect } from "@/shared/status";
import { BOOK_STATUS, useBookMutations, type Book } from "./api";

// Widths on <th> (table-fixed). `narrow` columns hide below lg (window < 1024px) so the minimum window still fits.
const COLS: { label: string; className: string }[] = [
  { label: "书名", className: "w-[28%] pl-4" },
  { label: "作者", className: "w-[18%]" },
  { label: "国家", className: "w-[10%] max-lg:hidden" },
  { label: "状态", className: "w-[84px]" },
  { label: "尝尝咸淡", className: "w-[104px]" },
  { label: "完成", className: "w-[84px]" },
  { label: "备注", className: "max-lg:hidden" },
];

export function BookTable(props: { books: Book[]; selectedId: number | null; onSelect: (id: number) => void }) {
  const { quickUpdate } = useBookMutations();
  const muted = "text-muted-foreground";
  return (
    <div className="rounded-card border bg-card">
      <table className="w-full table-fixed text-sm">
        <thead>
          <tr className="border-b text-left text-xs text-muted-foreground">
            {COLS.map((c) => (
              <th key={c.label} className={`px-3 py-2 font-normal ${c.className}`}>
                {c.label}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {props.books.map((b) => {
            const selected = b.id === props.selectedId;
            return (
              <tr
                key={b.id}
                onClick={() => props.onSelect(b.id)}
                className={`h-10 cursor-pointer border-b transition-colors duration-150 last:border-b-0 ${
                  selected ? "bg-accent" : "hover:bg-muted/70"
                }`}
              >
                <td className="truncate px-3 pl-4">{b.title}</td>
                <td className={`truncate px-3 ${muted}`}>{b.author ?? "—"}</td>
                <td className={`truncate px-3 max-lg:hidden ${muted}`}>{b.country ?? "—"}</td>
                <td className="px-3">
                  <StatusSelect
                    value={b.status}
                    labels={BOOK_STATUS}
                    onChange={(status) => quickUpdate.mutate({ id: b.id, patch: { status } })}
                  />
                </td>
                <td className="px-2">
                  <StarRating value={b.rating} onChange={(rating) => quickUpdate.mutate({ id: b.id, patch: { rating } })} />
                </td>
                <td className={`px-3 ${muted}`}>{b.finishedMonth ?? "—"}</td>
                <td className={`truncate px-3 max-lg:hidden ${muted}`}>{b.note ?? ""}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
