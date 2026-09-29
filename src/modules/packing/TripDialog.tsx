// Create a trip (optionally from a template: its items are copied) / edit / delete a trip.
import { useEffect, useRef, useState, type FormEvent } from "react";
import { Trash2 } from "lucide-react";
import { Dropdown } from "@/shared/Dropdown";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { Button, Field, inputClass, TextArea, TextInput } from "@/shared/ui";
import { usePackingMutations, type Template, type Trip } from "./api";

export function TripDialog(props: {
  trip: Trip | null; // null = create
  templates: Template[];
  presetTemplateId?: number | null;
  onClose: () => void;
  onCreated: (t: Trip) => void;
  onDeleted: () => void;
}) {
  const { trip, templates } = props;
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const d = ref.current;
    if (d && !d.open) d.showModal();
  }, []);
  const firstTemplate = props.presetTemplateId ?? templates[0]?.id ?? null;
  const [templateId, setTemplateId] = useState<number | null>(firstTemplate);
  const [name, setName] = useState(trip?.name ?? templates.find((t) => t.id === firstTemplate)?.name ?? "");
  const [nameTouched, setNameTouched] = useState(!!trip);
  const [departOn, setDepartOn] = useState(trip?.departOn ?? "");
  const [note, setNote] = useState(trip?.note ?? "");
  const [error, setError] = useState<string | null>(null);
  const m = usePackingMutations();
  const toast = useToast();
  const [confirm, confirmDialog] = useConfirm();

  const pickTemplate = (id: number | null) => {
    setTemplateId(id);
    if (!nameTouched) setName(templates.find((t) => t.id === id)?.name ?? "");
  };

  const save = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    const patch = { name, departOn: departOn || null, note: note.trim() || null };
    try {
      if (trip) {
        await m.updateTrip.mutateAsync({ id: trip.id, patch });
        toast("已保存");
      } else {
        const t = await m.createTrip.mutateAsync({ patch, templateId });
        toast(t.total > 0 ? `已新建「${t.name}」，清单里有 ${t.total} 件物品` : `已新建「${t.name}」`);
        props.onCreated(t);
      }
      props.onClose();
    } catch (err) {
      setError(errorText(err));
    }
  };

  const del = async () => {
    if (!trip) return;
    const ok = await confirm({
      title: `删除「${trip.name}」？`,
      message: "这次出行的清单会一起删除，模板不受影响。删除后无法撤销，只能从备份里找回。",
      confirmLabel: "删除",
      danger: true,
    });
    if (!ok) return;
    try {
      await m.deleteTrip.mutateAsync(trip.id);
      toast(`已删除「${trip.name}」`);
      props.onDeleted();
      props.onClose();
    } catch (err) {
      setError(errorText(err));
    }
  };

  return (
    <dialog
      ref={ref}
      onCancel={(e) => {
        e.preventDefault();
        props.onClose();
      }}
      className="m-auto h-fit w-[420px] rounded-card border bg-popover p-5 text-foreground shadow-xl backdrop:bg-black/30"
    >
      <form onSubmit={save} className="space-y-4">
        <p className="text-base font-bold">{trip ? "编辑出行" : "新出行"}</p>
        {!trip && (
          <Field label="清单从哪来">
            <div>
              <Dropdown
                ariaLabel="模板"
                value={templateId}
                onChange={pickTemplate}
                options={[
                  ...templates.map((t) => ({ value: t.id as number | null, label: `${t.name}（${t.itemCount} 件）` })),
                  { value: null, label: "空白清单" },
                ]}
              />
            </div>
          </Field>
        )}
        <Field label="名称">
          <TextInput
            autoFocus
            value={name}
            onChange={(e) => {
              setName(e.target.value);
              setNameTouched(true);
            }}
            placeholder="比如：2026 国庆 · 回家"
          />
        </Field>
        <Field label="出发日期">
          <input type="date" value={departOn} onChange={(e) => setDepartOn(e.target.value)} className={inputClass} aria-label="出发日期" />
        </Field>
        <Field label="备注">
          <TextArea rows={2} value={note} onChange={(e) => setNote(e.target.value)} />
        </Field>
        {trip?.templateName && <p className="text-xs text-muted-foreground">清单来自模板「{trip.templateName}」</p>}
        {error && <p className="text-sm text-destructive">{error}</p>}
        <div className="flex items-center gap-2 pt-1">
          {trip && (
            <Button variant="ghost-danger" onClick={() => void del()}>
              <Trash2 size={14} /> 删除出行
            </Button>
          )}
          <div className="flex-1" />
          <Button onClick={props.onClose}>取消</Button>
          <Button type="submit" variant="primary">
            {trip ? "保存" : "新建"}
          </Button>
        </div>
      </form>
      {confirmDialog}
    </dialog>
  );
}
