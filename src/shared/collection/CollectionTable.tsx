import { StarRating } from "@/shared/StarRating";
import { StatusSelect } from "@/shared/status";
import { useKinds } from "@/shared/kinds/api";
import { useItemMutations } from "./api";
import type { CollectionApi, CollectionConfig, Item } from "./types";

// Widths on <th> (table-fixed). max-lg:hidden columns drop below 1024px so the minimum window still fits.
export function CollectionTable(props: {
  config: CollectionConfig;
  api: CollectionApi;
  items: Item[];
  selectedId: number | null;
  onSelect: (id: number) => void;
}) {
  const { config } = props;
  const { quickUpdate } = useItemMutations(props.api);
  const { data: kinds = [] } = useKinds(config.moduleId);
  const kindName = new Map(kinds.map((k) => [k.id, k.name]));
  const cols: { label: string; className: string }[] = [
    { label: config.titleLabel, className: "w-[26%] pl-4" },
    { label: config.creatorLabel, className: "w-[16%]" },
    { label: "类型", className: "w-[76px]" },
    { label: "国家", className: "w-[10%] max-lg:hidden" },
    { label: "状态", className: "w-[84px]" },
    { label: "尝尝咸淡", className: "w-[104px]" },
    { label: "完成", className: "w-[84px]" },
    { label: "备注", className: "max-lg:hidden" },
  ];
  const muted = "text-muted-foreground";
  return (
    <div className="rounded-card border bg-card">
      <table className="w-full table-fixed text-sm">
        <thead>
          <tr className="border-b text-left text-xs text-muted-foreground">
            {cols.map((c) => (
              <th key={c.label} className={`px-3 py-2 font-normal ${c.className}`}>
                {c.label}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {props.items.map((it) => (
            <tr
              key={it.id}
              onClick={() => props.onSelect(it.id)}
              className={`h-10 cursor-pointer border-b transition-colors duration-150 last:border-b-0 ${
                it.id === props.selectedId ? "bg-accent" : "hover:bg-muted/70"
              }`}
            >
              <td className="truncate px-3 pl-4">{it.title}</td>
              <td className={`truncate px-3 ${muted}`}>{it.creator ?? "—"}</td>
              <td className={`truncate px-3 ${muted}`}>{(it.kindId !== null && kindName.get(it.kindId)) || "—"}</td>
              <td className={`truncate px-3 max-lg:hidden ${muted}`}>{it.country ?? "—"}</td>
              <td className="px-3">
                <StatusSelect
                  value={it.status}
                  labels={config.statusLabels}
                  onChange={(status) => quickUpdate.mutate({ id: it.id, patch: { status } })}
                />
              </td>
              <td className="px-2">
                <StarRating value={it.rating} onChange={(rating) => quickUpdate.mutate({ id: it.id, patch: { rating } })} />
              </td>
              <td className={`px-3 whitespace-nowrap ${muted}`}>{it.finishedMonth ?? "—"}</td>
              <td className={`truncate px-3 max-lg:hidden ${muted}`}>{it.note ?? ""}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
