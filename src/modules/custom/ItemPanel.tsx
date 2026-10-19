// Side panel for one 五花八门 item: 名称, then every field (shown in the table or not) by its type.
import { useEffect, useMemo, useState, type FormEvent } from "react";
import { ExternalLink } from "lucide-react";
import { Dropdown } from "@/shared/Dropdown";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { api as ipc } from "@/shared/ipc";
import { SidePanel } from "@/shared/panel";
import { YearInput } from "@/shared/MonthInput";
import { StarRating } from "@/shared/StarRating";
import { Tag, tagStyle } from "@/shared/tags";
import { Field as Row, IconButton, Switch, TextArea, TextInput } from "@/shared/ui";
import { useCustomMutations, type Field, type Item, type Value } from "./api";

const pad = (n: number) => String(n).padStart(2, "0");
const stamp = (s: string) => {
  const d = new Date(s);
  return `${d.getFullYear()} / ${pad(d.getMonth() + 1)} / ${pad(d.getDate())}`;
};

/** The editor's value for a field: text inputs hold strings (a number is typed as text). */
type Draft = { name: string; values: Record<string, Value | null> };

function initial(fields: Field[], item: Item | null): Draft {
  const values: Record<string, Value | null> = {};
  for (const f of fields) {
    const v = item?.values[f.id];
    values[f.id] = v === undefined ? null : f.type === "number" ? String(v) : v;
  }
  return { name: item?.name ?? "", values };
}

function Editor(props: { f: Field; value: Value | null; onChange: (v: Value | null) => void }) {
  const { f, value: v } = props;
  const toast = useToast();
  switch (f.type) {
    case "longtext":
      return <TextArea aria-label={f.name} rows={4} value={typeof v === "string" ? v : ""} onChange={(e) => props.onChange(e.target.value)} maxLength={5000} />;
    case "number":
      return <TextInput aria-label={f.name} inputMode="decimal" value={typeof v === "string" ? v : ""} onChange={(e) => props.onChange(e.target.value)} className="w-40" />;
    case "year":
      return <YearInput label={f.name} from={1900} value={typeof v === "string" ? v : null} onChange={(y) => props.onChange(y)} />;
    case "select":
      return (
        <Dropdown<string | null>
          ariaLabel={f.name}
          value={typeof v === "string" ? v : null}
          onChange={(o) => props.onChange(o)}
          placeholder="不选"
          menuMaxHeight={280}
          options={[{ value: null, label: <span className="text-muted-foreground">不选</span> }, ...f.options.map((o, i) => ({ value: o as string | null, label: <Tag index={i}>{o}</Tag> }))]}
        />
      );
    case "multi": {
      const picked = Array.isArray(v) ? v : [];
      return (
        <div className="flex flex-wrap gap-1.5">
          {f.options.map((o, i) => {
            const on = picked.includes(o);
            return (
              <button
                key={o}
                type="button"
                aria-pressed={on}
                onClick={() => {
                  const next = on ? picked.filter((x) => x !== o) : f.options.filter((x) => x === o || picked.includes(x));
                  props.onChange(next.length ? next : null);
                }}
                style={on ? tagStyle(i) : undefined}
                className={`rounded-full border px-2.5 py-0.5 text-xs transition-colors duration-150 outline-none focus-visible:ring-2 focus-visible:ring-ring ${on ? "border-transparent" : "text-muted-foreground hover:bg-muted"}`}
              >
                {o}
              </button>
            );
          })}
        </div>
      );
    }
    case "check":
      return <Switch label={f.name} checked={v === true} onChange={(on) => props.onChange(on ? true : null)} />;
    case "rating":
      return <StarRating size={20} value={typeof v === "number" ? v : null} onChange={(n) => props.onChange(n)} />;
    case "link":
      return (
        <div className="flex items-center gap-1">
          <TextInput aria-label={f.name} value={typeof v === "string" ? v : ""} onChange={(e) => props.onChange(e.target.value)} maxLength={1000} />
          <IconButton label="打开链接" disabled={typeof v !== "string" || !v.trim()} onClick={() => void ipc.openLink(String(v)).catch((err) => toast(errorText(err), "error"))}>
            <ExternalLink size={14} />
          </IconButton>
        </div>
      );
    default:
      return <TextInput aria-label={f.name} value={typeof v === "string" ? v : ""} onChange={(e) => props.onChange(e.target.value)} maxLength={200} />;
  }
}

type Props = { moduleId: number; fields: Field[]; item: Item | null; onClose: () => void; onDone: () => void; onDirtyChange: (dirty: boolean) => void };

export function ItemPanel(props: Props) {
  const it = props.item;
  const start = useMemo(() => initial(props.fields, it), [it, props.fields]);
  const [d, setD] = useState(start);
  const [error, setError] = useState<string | null>(null);
  const m = useCustomMutations();
  const toast = useToast();
  const [confirm, confirmDialog] = useConfirm();
  const dirty = JSON.stringify(d) !== JSON.stringify(start);
  useEffect(() => {
    props.onDirtyChange(dirty);
  }, [dirty]); // eslint-disable-line react-hooks/exhaustive-deps

  const save = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    // numbers go as typed (Rust reads them); empty → null
    const values = Object.fromEntries(Object.entries(d.values).map(([k, v]) => [k, typeof v === "string" && !v.trim() ? null : v]));
    try {
      if (it) {
        if (dirty) await m.itemUpdate.mutateAsync({ id: it.id, patch: { name: d.name, values } });
        toast("已保存");
      } else {
        await m.itemCreate.mutateAsync({ moduleId: props.moduleId, patch: { name: d.name, values } });
        toast(`已添加「${d.name.trim()}」`);
      }
      props.onDone();
    } catch (err) {
      setError(errorText(err));
    }
  };
  const del = async () => {
    if (!it || !(await confirm({ title: `删除「${it.name}」？`, confirmLabel: "删除", danger: true }))) return;
    m.itemDelete.mutate(it.id, {
      onSuccess: () => {
        toast("已删除");
        props.onDone();
      },
    });
  };

  return (
    <SidePanel
      title={it ? "编辑条目" : "新条目"}
      onClose={props.onClose}
      onSubmit={save}
      onDelete={it ? () => void del() : undefined}
      error={error}
      meta={it ? `添加于 ${stamp(it.createdAt)} · 修改于 ${stamp(it.updatedAt)}` : undefined}
      extra={confirmDialog}
    >
      <Row label="名称">
        <TextInput autoFocus value={d.name} onChange={(e) => setD({ ...d, name: e.target.value })} placeholder="必填" maxLength={200} />
      </Row>
      {props.fields.map((f) => (
        <Row key={f.id} label={f.name}>
          <Editor f={f} value={d.values[f.id] ?? null} onChange={(v) => setD((x) => ({ ...x, values: { ...x.values, [f.id]: v } }))} />
        </Row>
      ))}
    </SidePanel>
  );
}
