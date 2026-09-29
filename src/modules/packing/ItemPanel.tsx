// Side panel for a template item or trip item. Create mode stays open after each save
// (clears the name, keeps the 类别) so a list can be typed in quickly.
import { useEffect, useMemo, useRef, useState, type FormEvent } from "react";
import { Dropdown } from "@/shared/Dropdown";
import { errorText, useToast } from "@/shared/feedback";
import { useKinds, UNCATEGORIZED } from "@/shared/kinds/api";
import { SidePanel } from "@/shared/panel";
import { Field, Segmented, TextInput } from "@/shared/ui";
import { usePackingMutations, type ListKind, type PackItem } from "./api";

type Draft = { name: string; kindId: number | null; note: string; packed: boolean };

export function ItemPanel(props: {
  list: ListKind;
  parentId: number;
  item: PackItem | null; // null = create
  onClose: () => void;
  onDone: () => void;
  onDirtyChange: (dirty: boolean) => void;
  onManageKinds: () => void;
}) {
  const { item, list } = props;
  const isNew = item === null;
  const { data: kinds = [] } = useKinds("packing");
  const defaultKindId = kinds[0]?.id ?? null;
  const initial = useMemo<Draft>(
    () => ({ name: item?.name ?? "", kindId: item ? item.kindId : defaultKindId, note: item?.note ?? "", packed: item?.packed ?? false }),
    [item, defaultKindId],
  );
  const [d, setD] = useState(initial);
  const [error, setError] = useState<string | null>(null);
  const [added, setAdded] = useState(0);
  const nameRef = useRef<HTMLInputElement>(null);
  const m = usePackingMutations();
  const toast = useToast();

  useEffect(() => {
    if (isNew) setD((x) => (x.kindId === null ? { ...x, kindId: defaultKindId } : x));
  }, [isNew, defaultKindId]);

  const dirty = isNew ? d.name.trim() !== "" || d.note.trim() !== "" : JSON.stringify(d) !== JSON.stringify(initial);
  useEffect(() => {
    props.onDirtyChange(dirty);
  }, [dirty]); // eslint-disable-line react-hooks/exhaustive-deps

  const set = <K extends keyof Draft>(k: K, v: Draft[K]) => setD((x) => ({ ...x, [k]: v }));

  const save = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    const patch = {
      name: d.name,
      kindId: d.kindId,
      note: d.note.trim() || null,
      ...(list === "trip" ? { packed: d.packed } : {}),
    };
    try {
      if (isNew) {
        await m.createItem.mutateAsync({ list, parentId: props.parentId, patch });
        toast(`已添加「${d.name.trim()}」`);
        setD((x) => ({ ...x, name: "", note: "", packed: false })); // keep 类别, keep going
        setAdded((n) => n + 1);
        nameRef.current?.focus();
        return;
      }
      if (dirty) await m.updateItem.mutateAsync({ list, id: item.id, patch });
      toast("已保存");
      props.onDone();
    } catch (err) {
      setError(errorText(err));
    }
  };

  const del = async () => {
    if (!item) return;
    try {
      await m.quickDeleteItem.mutateAsync({ list, id: item.id });
      toast(`已删除「${item.name}」`);
      props.onDone();
    } catch (err) {
      setError(errorText(err));
    }
  };

  return (
    <SidePanel
      title={isNew ? "添加物品" : "编辑物品"}
      onClose={props.onClose}
      onSubmit={save}
      onDelete={item ? () => void del() : undefined}
      cancelLabel={isNew && added > 0 ? "完成" : "取消"}
      error={error}
      meta={isNew ? (added > 0 ? `已添加 ${added} 件。按回车继续添加下一件。` : "保存后可以接着添加下一件。") : undefined}
    >
      <Field label="物品">
        <TextInput ref={nameRef} autoFocus value={d.name} onChange={(e) => set("name", e.target.value)} placeholder="必填" maxLength={100} />
      </Field>
      <Field label="类别">
        <div>
          <Dropdown
            ariaLabel="类别"
            value={d.kindId}
            onChange={(v) => set("kindId", v)}
            options={[...kinds.map((k) => ({ value: k.id as number | null, label: k.name })), { value: null, label: UNCATEGORIZED }]}
            actions={[{ label: "管理类别…", onSelect: props.onManageKinds }]}
          />
        </div>
      </Field>
      <Field label="备注">
        <TextInput value={d.note} onChange={(e) => set("note", e.target.value)} placeholder="比如：飞机须托运" maxLength={1000} />
      </Field>
      {list === "trip" && (
        <Field label="这次">
          <Segmented
            ariaLabel="是否已打包"
            value={d.packed ? "yes" : "no"}
            onChange={(v) => set("packed", v === "yes")}
            options={[
              { value: "no", label: "还没装" },
              { value: "yes", label: "已装好" },
            ]}
          />
        </Field>
      )}
    </SidePanel>
  );
}
