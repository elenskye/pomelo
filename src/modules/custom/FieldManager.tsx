// 管理字段 for a 五花八门 list: the fields in order (drag), each with its type, 「表格里显示」 and 编辑 / 删除;
// 新字段 at the bottom. The field dialog edits the name, the type (new fields only) and 单选 / 多选 options
// (renaming one renames it in every item; removing one takes it out of them).
import { useEffect, useRef, useState, type FormEvent } from "react";
import { Pencil, Plus, Trash2, X } from "lucide-react";
import { Dropdown } from "@/shared/Dropdown";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { Modal } from "@/shared/Modal";
import { DragHandle, useSortable } from "@/shared/sortable";
import { Button, Field as Row, IconButton, Switch, TextInput } from "@/shared/ui";
import { FIELD_TYPES, fieldTypeLabel, MAX_FIELDS, MAX_OPTIONS, useCustomMutations, type Field, type FieldType } from "./api";

type Opt = { key: number; name: string; was: string | null };
let keySeq = 0;

function FieldDialog(props: { moduleId: number; field: Field | null; onClose: () => void }) {
  const f = props.field;
  const [name, setName] = useState(f?.name ?? "");
  const [type, setType] = useState<FieldType>(f?.type ?? "text");
  const [opts, setOpts] = useState<Opt[]>(() => (f?.options ?? []).map((o) => ({ key: ++keySeq, name: o, was: o })));
  const [error, setError] = useState<string | null>(null);
  const m = useCustomMutations();
  const toast = useToast();
  const hasOptions = type === "select" || type === "multi";
  const last = useRef<HTMLInputElement>(null);
  const [focusLast, setFocusLast] = useState(false);
  useEffect(() => {
    if (focusLast) last.current?.focus();
    setFocusLast(false);
  }, [focusLast]);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    const options = hasOptions ? opts.filter((o) => o.name.trim()).map((o) => ({ name: o.name, was: o.was })) : undefined;
    try {
      if (f) await m.fieldUpdate.mutateAsync({ id: f.id, patch: { name, options } });
      else await m.fieldCreate.mutateAsync({ moduleId: props.moduleId, patch: { name, type, options } });
      toast(f ? "已保存" : `已添加字段「${name.trim()}」`);
      props.onClose();
    } catch (err) {
      setError(errorText(err));
    }
  };

  return (
    <Modal title={f ? "编辑字段" : "新字段"} onClose={props.onClose} onSubmit={submit} error={error} submitLabel={f ? "保存" : "添加"}>
      <Row label="字段名">
        <TextInput autoFocus value={name} onChange={(e) => setName(e.target.value)} placeholder="必填" maxLength={20} />
      </Row>
      <Row label="类型">
        {f ? <p className="text-sm">{fieldTypeLabel(f.type)}</p> : <Dropdown<FieldType> ariaLabel="类型" value={type} onChange={setType} options={FIELD_TYPES} menuMaxHeight={360} />}
      </Row>
      {hasOptions && (
        <Row label="选项">
          <div className="space-y-1.5">
            {opts.map((o, i) => (
              <div key={o.key} className="flex items-center gap-1">
                <TextInput
                  ref={i === opts.length - 1 ? last : undefined}
                  aria-label={`选项 ${i + 1}`}
                  value={o.name}
                  maxLength={20}
                  onChange={(e) => setOpts(opts.map((x) => (x.key === o.key ? { ...x, name: e.target.value } : x)))}
                />
                <IconButton label="删掉这个选项" danger onClick={() => setOpts(opts.filter((x) => x.key !== o.key))}>
                  <X size={14} />
                </IconButton>
              </div>
            ))}
            {opts.length < MAX_OPTIONS && (
              <button
                type="button"
                onClick={() => {
                  setOpts([...opts, { key: ++keySeq, name: "", was: null }]);
                  setFocusLast(true);
                }}
                className="flex items-center gap-1 text-sm text-primary hover:underline"
              >
                <Plus size={14} /> 加一个选项
              </button>
            )}
          </div>
        </Row>
      )}
    </Modal>
  );
}

export function FieldManager(props: { moduleId: number; fields: Field[]; itemCount: number; onClose: () => void }) {
  const ref = useRef<HTMLDialogElement>(null);
  const m = useCustomMutations();
  const toast = useToast();
  const [confirm, confirmDialog] = useConfirm();
  const [editing, setEditing] = useState<Field | "new" | null>(null);
  const sort = useSortable((ids) => m.fieldsReorder.mutate({ moduleId: props.moduleId, ids }));
  useEffect(() => {
    const d = ref.current;
    if (d && !d.open) d.showModal();
  }, []);
  const del = async (f: Field) => {
    const ok = await confirm({ title: `删除字段「${f.name}」？`, message: props.itemCount > 0 ? "每一条里填的这一项也会一起删掉。" : undefined, confirmLabel: "删除", danger: true });
    if (ok) m.fieldDelete.mutate(f.id, { onSuccess: () => toast("已删除") });
  };

  return (
    <dialog
      ref={ref}
      onCancel={(e) => {
        e.preventDefault();
        if (!editing) props.onClose();
      }}
      className="m-auto h-fit max-h-[85vh] w-[520px] rounded-card border bg-popover p-5 text-foreground shadow-xl backdrop:bg-black/30"
    >
      <p className="pb-3 text-base font-bold">管理字段</p>
      <div className="flex items-center gap-2.5 border-b px-1 pb-2 text-sm">
        <span className="w-4" />
        <span className="flex-1">名称</span>
        <span className="text-xs text-muted-foreground">每一条都有</span>
      </div>
      <div ref={sort.listRef} className="-m-1 max-h-[50vh] overflow-y-auto p-1">
        {props.fields.map((f) => (
          <div key={f.id} {...sort.item(f.id)} className="group flex items-center gap-2.5 rounded-control bg-popover px-1 py-1.5 text-sm">
            <DragHandle {...sort.handle(f.id)} />
            <span className="min-w-0 flex-1 truncate">{f.name}</span>
            <span className="w-14 shrink-0 text-xs text-muted-foreground">{fieldTypeLabel(f.type)}</span>
            <span className="flex shrink-0 items-center gap-1.5 text-xs text-muted-foreground">
              表格里显示
              <Switch label={`「${f.name}」在表格里显示`} checked={f.shown} onChange={(shown) => m.fieldUpdate.mutate({ id: f.id, patch: { shown } }, { onError: (e) => toast(errorText(e), "error") })} />
            </span>
            <IconButton label={`编辑「${f.name}」`} onClick={() => setEditing(f)}>
              <Pencil size={14} />
            </IconButton>
            <IconButton label={`删除「${f.name}」`} danger onClick={() => void del(f)}>
              <Trash2 size={14} />
            </IconButton>
          </div>
        ))}
      </div>
      <div className="flex items-center gap-2 pt-4">
        {props.fields.length < MAX_FIELDS && (
          <Button onClick={() => setEditing("new")}>
            <Plus size={14} /> 新字段
          </Button>
        )}
        <div className="flex-1" />
        <Button variant="primary" onClick={props.onClose}>
          完成
        </Button>
      </div>
      {editing && <FieldDialog moduleId={props.moduleId} field={editing === "new" ? null : editing} onClose={() => setEditing(null)} />}
      {confirmDialog}
    </dialog>
  );
}
