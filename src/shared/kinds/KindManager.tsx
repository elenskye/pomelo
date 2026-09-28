// Modal for editing a module's 类型 list: rename inline, reorder ↑↓, delete (items → 未分类), add.
import { useEffect, useRef, useState, type FormEvent } from "react";
import { ArrowDown, ArrowUp, Plus, Trash2 } from "lucide-react";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { Button, IconButton, TextInput } from "@/shared/ui";
import { useKindMutations, useKinds, type Kind } from "./api";

function KindName(props: { kind: Kind; onSave: (name: string) => Promise<boolean> }) {
  const [v, setV] = useState(props.kind.name);
  useEffect(() => setV(props.kind.name), [props.kind.name]);
  const commit = async () => {
    const name = v.trim();
    if (!name || name === props.kind.name) return setV(props.kind.name);
    if (!(await props.onSave(name))) setV(props.kind.name);
  };
  return (
    <input
      value={v}
      aria-label={`类型名称：${props.kind.name}`}
      spellCheck={false}
      onChange={(e) => setV(e.target.value)}
      onBlur={() => void commit()}
      onKeyDown={(e) => {
        if (e.key === "Enter") {
          e.preventDefault();
          e.currentTarget.blur();
        } else if (e.key === "Escape") {
          e.preventDefault(); // keep the dialog open; just undo the edit
          e.stopPropagation();
          setV(props.kind.name);
        }
      }}
      className="min-w-0 flex-1 rounded-md bg-transparent px-2 py-1.5 text-sm outline-none hover:bg-card focus:bg-card focus:ring-2 focus:ring-ring"
    />
  );
}

export function KindManager(props: {
  module: string;
  moduleLabel: string;
  unit: string; // 本 / 部 / 节
  itemsKey: readonly unknown[];
  onClose: () => void;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const d = ref.current;
    if (d && !d.open) d.showModal();
  }, []);
  const { data: kinds = [] } = useKinds(props.module);
  const m = useKindMutations(props.module, props.itemsKey);
  const toast = useToast();
  const [confirm, confirmDialog] = useConfirm();
  const [newName, setNewName] = useState("");

  const run = async (p: Promise<unknown>) => {
    try {
      await p;
      return true;
    } catch (e) {
      toast(errorText(e), "error");
      return false;
    }
  };
  const move = (i: number, d: -1 | 1) => {
    const ids = kinds.map((k) => k.id);
    [ids[i], ids[i + d]] = [ids[i + d], ids[i]];
    void run(m.reorder.mutateAsync(ids));
  };
  const remove = async (k: Kind) => {
    const ok =
      k.count === 0 ||
      (await confirm({
        title: `删除类型「${k.name}」？`,
        message: `有 ${k.count} ${props.unit}属于这个类型。删除后它们会变成「未分类」，不会被删掉。`,
        confirmLabel: "删除",
        danger: true,
      }));
    if (ok) void run(m.remove.mutateAsync(k.id));
  };
  const add = async (e: FormEvent) => {
    e.preventDefault();
    if (newName.trim() && (await run(m.create.mutateAsync(newName)))) setNewName("");
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
      <div className="flex items-baseline justify-between">
        <p className="text-base font-bold">管理类型</p>
        <span className="text-xs text-muted-foreground">{props.moduleLabel}</span>
      </div>
      <p className="mt-1 text-xs text-muted-foreground">点名字可以直接改名，按回车保存。</p>

      <ul className="mt-3 max-h-80 space-y-0.5 overflow-y-auto">
        {kinds.map((k, i) => (
          <li key={k.id} className="flex items-center gap-1 rounded-control pr-1 hover:bg-muted/60">
            <KindName kind={k} onSave={(name) => run(m.rename.mutateAsync({ id: k.id, name }))} />
            <span className="w-14 shrink-0 text-right text-xs text-muted-foreground">
              {k.count} {props.unit}
            </span>
            <IconButton label="上移" disabled={i === 0} onClick={() => move(i, -1)}>
              <ArrowUp size={14} />
            </IconButton>
            <IconButton label="下移" disabled={i === kinds.length - 1} onClick={() => move(i, 1)}>
              <ArrowDown size={14} />
            </IconButton>
            <IconButton label={`删除「${k.name}」`} danger onClick={() => void remove(k)}>
              <Trash2 size={14} />
            </IconButton>
          </li>
        ))}
        {kinds.length === 0 && <li className="py-3 text-center text-sm text-muted-foreground">还没有类型</li>}
      </ul>

      <form onSubmit={add} className="mt-3 flex gap-2">
        <TextInput value={newName} onChange={(e) => setNewName(e.target.value)} placeholder="新类型的名字" maxLength={20} />
        <Button type="submit">
          <Plus size={14} /> 添加
        </Button>
      </form>

      <div className="mt-5 flex justify-end">
        <Button variant="primary" onClick={props.onClose}>
          完成
        </Button>
      </div>
      {confirmDialog}
    </dialog>
  );
}
