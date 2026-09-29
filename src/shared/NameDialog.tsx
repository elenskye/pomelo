// Modal to create / edit / delete a named container (课程, 模板…): name + note.
import { useEffect, useRef, useState, type FormEvent } from "react";
import { Trash2 } from "lucide-react";
import { errorText, useConfirm } from "./feedback";
import { Button, Field, TextArea, TextInput } from "./ui";

export function NameDialog(props: {
  title: string;
  nameLabel: string;
  initial: { name: string; note: string | null } | null; // null = create
  onSubmit: (v: { name: string; note: string | null }) => Promise<unknown>;
  onClose: () => void;
  /** Edit mode only. */
  remove?: { label: string; confirmTitle: string; confirmMessage: string; run: () => Promise<unknown> };
}) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const d = ref.current;
    if (d && !d.open) d.showModal();
  }, []);
  const [name, setName] = useState(props.initial?.name ?? "");
  const [note, setNote] = useState(props.initial?.note ?? "");
  const [error, setError] = useState<string | null>(null);
  const [confirm, confirmDialog] = useConfirm();

  const save = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    try {
      await props.onSubmit({ name, note: note.trim() || null });
      props.onClose();
    } catch (err) {
      setError(errorText(err));
    }
  };

  const del = async () => {
    const r = props.remove;
    if (!r) return;
    if (!(await confirm({ title: r.confirmTitle, message: r.confirmMessage, confirmLabel: "删除", danger: true }))) return;
    try {
      await r.run();
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
        <p className="text-base font-bold">{props.title}</p>
        <Field label={props.nameLabel}>
          <TextInput autoFocus value={name} onChange={(e) => setName(e.target.value)} placeholder="必填" />
        </Field>
        <Field label="备注">
          <TextArea rows={3} value={note} onChange={(e) => setNote(e.target.value)} />
        </Field>
        {error && <p className="text-sm text-destructive">{error}</p>}
        <div className="flex items-center gap-2 pt-1">
          {props.remove && (
            <Button variant="ghost-danger" onClick={() => void del()}>
              <Trash2 size={14} /> {props.remove.label}
            </Button>
          )}
          <div className="flex-1" />
          <Button onClick={props.onClose}>取消</Button>
          <Button type="submit" variant="primary">
            保存
          </Button>
        </div>
      </form>
      {confirmDialog}
    </dialog>
  );
}
