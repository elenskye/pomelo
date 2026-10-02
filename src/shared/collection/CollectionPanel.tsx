import { useEffect, useMemo, useState, type FormEvent } from "react";
import { Dropdown } from "@/shared/Dropdown";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { useKinds, UNCATEGORIZED } from "@/shared/kinds/api";
import { currentMonth, MonthInput } from "@/shared/MonthInput";
import { SidePanel } from "@/shared/panel";
import { StarRating } from "@/shared/StarRating";
import { STATUS_ORDER, type StatusCode } from "@/shared/status";
import { Field, Segmented, TextArea, TextInput } from "@/shared/ui";
import { useItemMutations } from "./api";
import type { CollectionApi, CollectionConfig, Item, ItemPatch } from "./types";

type Draft = {
  title: string;
  creator: string;
  country: string;
  kindId: number | null;
  status: StatusCode;
  rating: number | null;
  finishedMonth: string | null;
  note: string;
};

const toDraft = (it: Item | null, defaultKindId: number | null): Draft => ({
  title: it?.title ?? "",
  creator: it?.creator ?? "",
  country: it?.country ?? "",
  kindId: it ? it.kindId : defaultKindId,
  status: it?.status ?? "unmarked",
  rating: it?.rating ?? null,
  finishedMonth: it?.finishedMonth ?? null,
  note: it?.note ?? "",
});

// Full form → every key explicit, so Rust won't auto-fill the month behind the user's back.
const toPatch = (d: Draft): ItemPatch => ({
  title: d.title,
  creator: d.creator.trim() || null,
  country: d.country.trim() || null,
  kindId: d.kindId,
  status: d.status,
  rating: d.rating,
  finishedMonth: d.finishedMonth,
  note: d.note.trim() || null,
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

  const set = <K extends keyof Draft>(k: K, v: Draft[K]) => setD((x) => ({ ...x, [k]: v }));
  const setStatus = (status: StatusCode) =>
    setD((x) => ({
      ...x,
      status,
      // mirror the Rust rule visibly: becoming done with no month → this month (user can change it)
      finishedMonth: status === "done" && x.status !== "done" && !x.finishedMonth ? currentMonth() : x.finishedMonth,
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
      <Field label={config.titleLabel}>
        <TextInput autoFocus value={d.title} onChange={(e) => set("title", e.target.value)} placeholder="必填" />
      </Field>
      <Field label={config.creatorLabel}>
        <TextInput
          list={listId("creators")}
          value={d.creator}
          onChange={(e) => set("creator", e.target.value)}
        />
      </Field>
      <Field label="类型">
        <div>
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
        <TextInput list={listId("countries")} value={d.country} onChange={(e) => set("country", e.target.value)} />
      </Field>
      <Field label="状态">
        <Segmented
          ariaLabel="状态"
          value={d.status}
          options={STATUS_ORDER.map((s) => ({ value: s, label: config.statusLabels[s] }))}
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
    </SidePanel>
  );
}
