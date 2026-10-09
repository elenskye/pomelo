// Modal for the status list: rename inline, reorder ↑↓, delete (only unused), add. Stages stay internal.
import { useEffect, useRef, useState, type FormEvent } from "react";
import { ArrowDown, ArrowUp, Plus, Trash2 } from "lucide-react";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { Button, IconButton, TextInput } from "@/shared/ui";
import { useJobMutations, useStatuses, type Status } from "./api";

function Name(props: { status: Status; onSave: (name: string) => void }) {
  const [v, setV] = useState(props.status.name);
  useEffect(() => setV(props.status.name), [props.status.name]);
  return (
    <input
      value={v}
      aria-label={`状态名：${props.status.name}`}
      onChange={(e) => setV(e.target.value)}
      onBlur={() => (v.trim() && v.trim() !== props.status.name ? props.onSave(v.trim()) : setV(props.status.name))}
      onKeyDown={(e) => {
        if (e.key === "Enter") {
          e.preventDefault();
          e.currentTarget.blur();
        } else if (e.key === "Escape") {
          e.preventDefault();
          e.stopPropagation();
          setV(props.status.name);
        }
      }}
      className="min-w-0 flex-1 rounded-md bg-transparent px-2 py-1.5 text-sm outline-none hover:bg-card focus:bg-card focus:ring-2 focus:ring-ring"
    />
  );
}

export function StatusManager(props: { onClose: () => void }) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const d = ref.current;
    if (d && !d.open) d.showModal();
  }, []);
  const { data: statuses = [] } = useStatuses();
  const m = useJobMutations();
  const toast = useToast();
  const [confirm, confirmDialog] = useConfirm();
  const [name, setName] = useState("");

  const move = (i: number, d: number) => {
    const ids = statuses.map((s) => s.id);
    [ids[i], ids[i + d]] = [ids[i + d], ids[i]];
    m.reorderStatuses.mutate(ids);
  };
  const add = async (e: FormEvent) => {
    e.preventDefault();
    if (!name.trim()) return;
    try {
      await m.createStatus.mutateAsync({ name });
      setName("");
    } catch (err) {
      toast(errorText(err), "error");
    }
  };
  const del = async (s: Status) => {
    if (await confirm({ title: `删除状态「${s.name}」？`, confirmLabel: "删除", danger: true })) m.deleteStatus.mutate(s.id);
  };

  return (
    <dialog
      ref={ref}
      onCancel={(e) => {
        e.preventDefault();
        props.onClose();
      }}
      className="m-auto h-fit w-[480px] rounded-card border bg-popover p-5 text-foreground shadow-xl backdrop:bg-black/30"
    >
      <p className="text-base font-bold">管理状态</p>
      <ul className="-mx-1 mt-3 max-h-96 space-y-0.5 overflow-y-auto p-1">
        {statuses.map((s, i) => (
          <li key={s.id} className="flex items-center gap-1 rounded-md bg-muted/60 pr-1">
            <Name status={s} onSave={(n) => m.updateStatus.mutate({ id: s.id, name: n })} />
            <span className="w-10 shrink-0 text-right text-xs text-muted-foreground tabular-nums">{s.count || ""}</span>
            <IconButton label="上移" disabled={i === 0} onClick={() => move(i, -1)}>
              <ArrowUp size={14} />
            </IconButton>
            <IconButton label="下移" disabled={i === statuses.length - 1} onClick={() => move(i, 1)}>
              <ArrowDown size={14} />
            </IconButton>
            <IconButton label={`删除「${s.name}」`} danger disabled={s.count > 0 || s.used > 0 || statuses.length <= 1} onClick={() => void del(s)}>
              <Trash2 size={14} />
            </IconButton>
          </li>
        ))}
      </ul>
      <form onSubmit={add} className="mt-3 flex items-center gap-2">
        <TextInput value={name} onChange={(e) => setName(e.target.value)} placeholder="新状态的名字" maxLength={20} />
        <Button type="submit" disabled={!name.trim()}>
          <Plus size={14} /> 添加
        </Button>
      </form>
      <div className="mt-4 flex justify-end">
        <Button variant="primary" onClick={props.onClose}>
          完成
        </Button>
      </div>
      {confirmDialog}
    </dialog>
  );
}
