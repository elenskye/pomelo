import { useEffect, useMemo, useState, type FormEvent } from "react";
import { Plus, Sparkles, X } from "lucide-react";
import { Dropdown } from "@/shared/Dropdown";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { useKinds, UNCATEGORIZED } from "@/shared/kinds/api";
import { currentYear, YearInput } from "@/shared/MonthInput";
import { SidePanel } from "@/shared/panel";
import { StarRating } from "@/shared/StarRating";
import { STATUS_ORDER, type StatusCode } from "@/shared/status";
import { Field, IconButton, Segmented, TextArea, TextInput } from "@/shared/ui";
import { Polish, QuickFill, assistApi, assistBusyText, useAssistOn, useFilled, type QuickFillResult } from "@/modules/picky/Assist";
import { usePickyStatus } from "@/modules/picky/api";
import { useItemMutations } from "./api";
import type { CollectionApi, CollectionConfig, Item, ItemPatch, Reread } from "./types";

type Draft = {
  title: string;
  creator: string;
  country: string;
  kindId: number | null;
  status: StatusCode;
  rating: number | null;
  finishedYear: string | null;
  note: string;
  rereads: Reread[];
};

const toDraft = (it: Item | null, defaultKindId: number | null): Draft => ({
  title: it?.title ?? "",
  creator: it?.creator ?? "",
  country: it?.country ?? "",
  kindId: it ? it.kindId : defaultKindId,
  status: it?.status ?? "unmarked",
  rating: it?.rating ?? null,
  finishedYear: it?.finishedYear ?? null,
  note: it?.note ?? "",
  rereads: it?.rereads ?? [],
});

// Full form → every key explicit, so Rust won't auto-fill the year behind the user's back.
const toPatch = (d: Draft): ItemPatch => ({
  title: d.title,
  creator: d.creator.trim() || null,
  country: d.country.trim() || null,
  kindId: d.kindId,
  status: d.status,
  rating: d.rating,
  finishedYear: d.finishedYear,
  note: d.note.trim() || null,
  rereads: d.rereads,
});

/** Right-side overlay editor. `item === null` = create mode. */
export function CollectionPanel(props: {
  config: CollectionConfig;
  api: CollectionApi;
  item: Item | null;
  creators: string[];
  countries: string[];
  onClose: () => void; // user-initiated (page checks unsaved changes)
  onDone: () => void; // after save/delete
  onDirtyChange: (dirty: boolean) => void;
  onManageKinds: () => void;
}) {
  const { config, item } = props;
  const isNew = item === null;
  const { data: kinds = [] } = useKinds(config.moduleId);
  const defaultKindId = kinds[0]?.id ?? null;
  const initial = useMemo(() => toDraft(item, defaultKindId), [item, defaultKindId]);
  const [d, setD] = useState(initial);
  const [error, setError] = useState<string | null>(null);
  const { create, update, remove } = useItemMutations(props.api);
  const toast = useToast();
  const [confirm, confirmDialog] = useConfirm();
  const listId = (f: string) => `${config.moduleId}-${f}`;

  // kinds may load after the panel opens: pick the default for a still-untouched new item
  useEffect(() => {
    if (isNew) setD((x) => (x.kindId === null ? { ...x, kindId: defaultKindId } : x));
  }, [isNew, defaultKindId]);

  const dirty = JSON.stringify(d) !== JSON.stringify(initial);
  useEffect(() => {
    props.onDirtyChange(dirty);
  }, [dirty]); // eslint-disable-line react-hooks/exhaustive-deps

  const filled = useFilled<keyof Draft>();
  const set = <K extends keyof Draft>(k: K, v: Draft[K]) => {
    filled.clear(k);
    setD((x) => ({ ...x, [k]: v }));
  };
  // 小拾 补全: 作者 / 导演、国家、类型 from the name (its own knowledge) into the empty fields
  const assistOn = useAssistOn();
  const status = usePickyStatus().data;
  const [filling, setFilling] = useState(false);
  const fillIn = async () => {
    setFilling(true);
    try {
      const f = await assistApi.fillWork(config.moduleId as "books" | "films", d.title);
      const next = { ...d };
      const keys: (keyof Draft)[] = [];
      if (f.creator && !d.creator.trim()) (next.creator = f.creator), keys.push("creator");
      if (f.country && !d.country.trim()) (next.country = f.country), keys.push("country");
      // a new item's kind is still the default until picked
      if (f.kindId != null && (d.kindId === null || (isNew && d.kindId === defaultKindId))) (next.kindId = f.kindId), keys.push("kindId");
      setD(next);
      filled.mark(keys);
      if (!keys.length) toast("没有能补的");
    } catch (err) {
      toast(errorText(err), "error");
    } finally {
      setFilling(false);
    }
  };
  // 一句话填好 (new items): 名称、作者 / 导演、国家、类型、状态、星级、完成时间
  const quick = isNew && assistOn;
  const quickFill = (f: QuickFillResult) => {
    const next = { ...d };
    const keys: (keyof Draft)[] = [];
    if (f.title) (next.title = f.title), keys.push("title");
    if (f.creator) (next.creator = f.creator), keys.push("creator");
    if (f.country) (next.country = f.country), keys.push("country");
    if (f.kindId != null) (next.kindId = f.kindId), keys.push("kindId");
    if (f.status) (next.status = f.status), keys.push("status");
    if (f.rating != null) (next.rating = f.rating), keys.push("rating");
    if (f.finishedYear) (next.finishedYear = f.finishedYear), keys.push("finishedYear");
    setD(next);
    filled.mark(keys);
  };
  const setStatus = (status: StatusCode) =>
    setD((x) => ({
      ...x,
      status,
      // mirror the Rust rule visibly: becoming done with no year → this year (user can change it)
      finishedYear: status === "done" && x.status !== "done" && !x.finishedYear ? currentYear() : x.finishedYear,
    }));

  const save = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    try {
      if (isNew) await create.mutateAsync(toPatch(d));
      else if (dirty) await update.mutateAsync({ id: item.id, patch: toPatch(d) });
      toast(isNew ? `已添加《${d.title.trim()}》` : "已保存");
      props.onDone();
    } catch (err) {
      setError(errorText(err));
    }
  };

  const del = async () => {
    if (!item) return;
    const ok = await confirm({
      title: `删除《${item.title}》？`,
      confirmLabel: "删除",
      danger: true,
    });
    if (!ok) return;
    try {
      await remove.mutateAsync(item.id);
      toast(`已删除《${item.title}》`);
      props.onDone();
    } catch (err) {
      setError(errorText(err));
    }
  };

  return (
    <SidePanel
      title={isNew ? config.newLabel : "编辑"}
      onClose={props.onClose}
      onSubmit={save}
      onDelete={item ? () => void del() : undefined}
      error={error}
      meta={item && `添加于 ${new Date(item.createdAt).toLocaleDateString("zh-CN")}`}
      extra={
        <>
          <datalist id={listId("creators")}>
            {props.creators.map((a) => (
              <option key={a} value={a} />
            ))}
          </datalist>
          <datalist id={listId("countries")}>
            {props.countries.map((c) => (
              <option key={c} value={c} />
            ))}
          </datalist>
          {confirmDialog}
        </>
      }
    >
      {quick && <QuickFill module={config.moduleId as "books" | "films"} onFill={quickFill} autoFocus />}
      <Field label={config.titleLabel}>
        <div className="flex items-center gap-1.5">
          <TextInput autoFocus={!quick} value={d.title} onChange={(e) => set("title", e.target.value)} placeholder="必填" className={`min-w-0 flex-1 ${filled.cls("title")}`} />
          {assistOn && d.title.trim() && (
            <IconButton label={filling ? assistBusyText(status, "正在补全…") : "补全"} disabled={filling} onClick={() => void fillIn()}>
              <Sparkles size={15} strokeWidth={1.75} className={filling ? "animate-pulse" : ""} />
            </IconButton>
          )}
        </div>
      </Field>
      <Field label={config.creatorLabel}>
        <TextInput
          list={listId("creators")}
          value={d.creator}
          onChange={(e) => set("creator", e.target.value)}
          className={filled.cls("creator")}
        />
      </Field>
      <Field label="类型">
        <div className={filled.halo("kindId")}>
          <Dropdown
            ariaLabel="类型"
            value={d.kindId}
            onChange={(v) => set("kindId", v)}
            options={[...kinds.map((k) => ({ value: k.id as number | null, label: k.name })), { value: null, label: UNCATEGORIZED }]}
            actions={[{ label: "管理类型…", onSelect: props.onManageKinds }]}
          />
        </div>
      </Field>
      <Field label="国家">
        <TextInput list={listId("countries")} value={d.country} onChange={(e) => set("country", e.target.value)} className={filled.cls("country")} />
      </Field>
      <Field label="状态">
        <div className={filled.halo("status")}>
          <Segmented
            ariaLabel="状态"
            value={d.status}
            options={STATUS_ORDER.map((s) => ({ value: s, label: config.statusLabels[s] }))}
            onChange={(v) => (filled.clear("status"), setStatus(v))}
          />
        </div>
      </Field>
      <Field label="尝尝咸淡">
        <div className={filled.halo("rating")}>
          <StarRating size={20} value={d.rating} onChange={(v) => set("rating", v)} />
        </div>
      </Field>
      <Field label="完成时间">
        <div className={filled.halo("finishedYear")}>
          <YearInput value={d.finishedYear} onChange={(v) => set("finishedYear", v)} />
        </div>
      </Field>
      {(d.status === "done" || d.rereads.length > 0) && (
        <Field label={config.rereadLabel}>
          <div className="space-y-2">
            {d.rereads.map((r, i) => {
              const put = (p: Partial<Reread>) => set("rereads", d.rereads.map((x, j) => (j === i ? { ...x, ...p } : x)));
              return (
                <div key={i} className="flex items-center gap-3">
                  <YearInput clearable={false} value={r.year} onChange={(year) => put({ year })} />
                  <StarRating size={16} value={r.rating} onChange={(rating) => put({ rating })} />
                  <span className="flex-1" />
                  <IconButton label={`删掉这次${config.rereadLabel}`} danger onClick={() => set("rereads", d.rereads.filter((_, j) => j !== i))}>
                    <X size={14} />
                  </IconButton>
                </div>
              );
            })}
            <button
              type="button"
              onClick={() => set("rereads", [...d.rereads, { year: currentYear(), rating: null }])}
              className="flex items-center gap-1 text-sm text-primary hover:underline"
            >
              <Plus size={14} /> {config.rereadAdd}
            </button>
          </div>
        </Field>
      )}
      <Field label="备注">
        <Polish module={config.moduleId} value={d.note} onChange={(v) => set("note", v)}>
          <TextArea rows={5} value={d.note} onChange={(e) => set("note", e.target.value)} />
        </Polish>
      </Field>
    </SidePanel>
  );
}
