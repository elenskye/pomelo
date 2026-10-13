// Create a trip (its list copied from a template or an earlier trip, or blank) / edit / delete a trip.
import { useEffect, useRef, useState, type FormEvent } from "react";
import { Trash2 } from "lucide-react";
import { Dropdown } from "@/shared/Dropdown";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { DateInput } from "@/shared/DateInput";
import { Button, Field, TextArea, TextInput } from "@/shared/ui";
import { usePackingMutations, type Template, type Trip } from "./api";
import { Polish, QuickFill, useAssistOn, useFilled, type QuickFillResult } from "@/modules/picky/Assist";

export function TripDialog(props: {
  trip: Trip | null; // null = create
  templates: Template[];
  trips: Trip[];
  presetTemplateId?: number | null;
  onClose: () => void;
  onCreated: (t: Trip) => void;
  onDeleted: () => void;
}) {
  const { trip, templates, trips } = props;
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const d = ref.current;
    if (d && !d.open) d.showModal();
  }, []);
  // list source: "t:<template id>" · "p:<trip id>" · "" blank
  const firstTemplate = props.presetTemplateId ?? templates[0]?.id ?? null;
  const [source, setSource] = useState(firstTemplate !== null ? `t:${firstTemplate}` : "");
  const sourceName = (v: string) => (v.startsWith("t:") ? templates : v.startsWith("p:") ? trips : []).find((x) => x.id === Number(v.slice(2)))?.name ?? "";
  const [name, setName] = useState(trip?.name ?? sourceName(source));
  const [nameTouched, setNameTouched] = useState(!!trip);
  const [departOn, setDepartOn] = useState(trip?.departOn ?? "");
  const [note, setNote] = useState(trip?.note ?? "");
  const [error, setError] = useState<string | null>(null);
  const m = usePackingMutations();
  const toast = useToast();
  const [confirm, confirmDialog] = useConfirm();

  const pickSource = (v: string) => {
    filled.clear("source");
    setSource(v);
    if (!nameTouched) setName(sourceName(v));
  };
  // 一句话填好 (new trips): 名称、出发日期 and the template that fits best
  const filled = useFilled<"name" | "departOn" | "source">();
  const assistOn = useAssistOn();
  const quick = !trip && assistOn;
  const fill = (f: QuickFillResult) => {
    const keys: ("name" | "departOn" | "source")[] = [];
    if (f.templateId != null && templates.some((t) => t.id === f.templateId)) setSource(`t:${f.templateId}`), keys.push("source");
    if (f.name) setName(f.name), setNameTouched(true), keys.push("name");
    if (f.departOn) setDepartOn(f.departOn), keys.push("departOn");
    filled.mark(keys);
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
        const id = source ? Number(source.slice(2)) : null;
        const t = await m.createTrip.mutateAsync({ patch, templateId: source.startsWith("t:") ? id : null, fromTripId: source.startsWith("p:") ? id : null });
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
      message: "这次出行的清单会一起删除，模板不受影响。",
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
        {quick && <QuickFill module="packing" onFill={fill} autoFocus />}
        {!trip && (
          <Field label="清单从哪来">
            <div className={filled.halo("source")}>
              <Dropdown
                ariaLabel="清单从哪来"
                value={source}
                onChange={pickSource}
                menuMaxHeight={300}
                options={[
                  { value: "", label: "空白清单" },
                  ...templates.map((t) => ({ value: `t:${t.id}`, label: `${t.name}（${t.itemCount} 件）`, group: "模板" })),
                  ...trips.map((t) => ({ value: `p:${t.id}`, label: `${t.name}（${t.total} 件）`, group: "以前的出行" })),
                ]}
              />
            </div>
          </Field>
        )}
        <Field label="名称">
          <TextInput
            autoFocus={!quick}
            value={name}
            onChange={(e) => {
              filled.clear("name");
              setName(e.target.value);
              setNameTouched(true);
            }}
            className={filled.cls("name")}
          />
        </Field>
        <Field label="出发日期">
          <div className={filled.halo("departOn")}>
            <DateInput
              clearable
              ariaLabel="出发日期"
              value={departOn}
              onChange={(v) => {
                filled.clear("departOn");
                setDepartOn(v);
              }}
            />
          </div>
        </Field>
        <Field label="备注">
          <Polish module="packing" value={note} onChange={setNote}>
            <TextArea rows={2} value={note} onChange={(e) => setNote(e.target.value)} />
          </Polish>
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
