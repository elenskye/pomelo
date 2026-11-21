import { StarRating } from "@/shared/StarRating";
import { StatusSelect } from "@/shared/status";
import { useKinds } from "@/shared/kinds/api";
import { TagSelect } from "@/shared/tags";
import { useItemMutations } from "./api";
import { latestYear, runName, type CollectionApi, type CollectionConfig, type Item } from "./types";

// Widths on <th> (table-fixed): every column has its own width but the name, which takes what is left; whatever is
// longer than its column ends in an ellipsis. max-lg:hidden columns drop below 1024px so the minimum window still fits.
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
  const { data: platforms = [] } = useKinds(config.platforms?.module ?? "", !!config.platforms);
  const kindName = new Map(kinds.map((k) => [k.id, k.name]));
  const platformName = new Map(platforms.map((k) => [k.id, k.name]));
  const cols: { label: string; className: string }[] = [
    { label: config.titleLabel, className: "pl-4" },
    ...(config.noCreator ? [] : [{ label: config.creatorLabel, className: "w-[16%]" }]),
    { label: "类型", className: config.kindTags ? "w-[108px]" : "w-[76px]" },
    ...(config.platforms ? [{ label: config.platforms.noun, className: "w-[96px]" }] : []),
    { label: "国家", className: "w-[80px] max-lg:hidden" },
    { label: "状态", className: "w-[96px]" },
    // five 20px stars and room on both sides
    { label: "尝尝咸淡", className: "w-[132px]" },
    { label: "完成", className: config.runs ? "w-[64px]" : "w-[84px]" },
    ...(config.runs ? [{ label: "周目", className: "w-[76px]" }] : []),
    { label: "备注", className: "w-[12%] max-lg:hidden" },
  ];
  const muted = "text-muted-foreground";
  return (
    // the rows stay off the hand-drawn edge (a picked row's tint would cover it)
    <div className="rounded-card border bg-card px-1 pb-1">
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
              {!config.noCreator && <td className={`truncate px-3 ${muted}`}>{it.creator ?? "—"}</td>}
              {config.kindTags ? (
                <td className="truncate px-3">
                  <TagSelect label="类型" options={kinds.map((k) => ({ value: k.id, name: k.name }))} value={it.kindId} onChange={(kindId) => quickUpdate.mutate({ id: it.id, patch: { kindId } })} />
                </td>
              ) : (
                <td className={`truncate px-3 ${muted}`}>{(it.kindId !== null && kindName.get(it.kindId)) || "—"}</td>
              )}
              {config.platforms && <td className={`truncate px-3 ${muted}`}>{(it.platformId != null && platformName.get(it.platformId)) || "—"}</td>}
              <td className={`truncate px-3 max-lg:hidden ${muted}`}>{it.country ?? "—"}</td>
              <td className="px-3">
                <StatusSelect
                  value={it.status}
                  labels={config.statusLabels}
                  order={config.statuses}
                  onChange={(status) => quickUpdate.mutate({ id: it.id, patch: { status } })}
                />
              </td>
              <td className="px-3">
                <StarRating value={it.rating} onChange={(rating) => quickUpdate.mutate({ id: it.id, patch: { rating } })} />
              </td>
              <td className={`truncate px-3 ${muted}`}>{latestYear(it) ?? "—"}</td>
              {config.runs && <td className={`truncate px-3 ${muted}`}>{it.runs ? runName(it.runs) : "—"}</td>}
              <td className={`truncate px-3 max-lg:hidden ${muted}`}>{it.note ?? ""}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
