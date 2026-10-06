// Left-column tag tree built from full paths (#a/b/c); siblings in pinyin order. Fold state is a UI
// setting; the active tag's ancestors always show. Rename (pencil on hover) rewrites the notes.
import { useState, type FormEvent } from "react";
import { ChevronDown, Hash, Pencil } from "lucide-react";
import { errorText, useToast } from "@/shared/feedback";
import { Modal } from "@/shared/Modal";
import { isStringArray, useSetting } from "@/shared/settings";
import { Field, TextInput } from "@/shared/ui";
import { useNoteMutations } from "./api";

type Node = { path: string; name: string; count: number; kids: Node[] };

const collator = new Intl.Collator("zh-Hans-CN-u-co-pinyin", { numeric: true });

export function buildTree(tags: { tag: string; count: number }[]): Node[] {
  const byPath = new Map<string, Node>();
  const roots: Node[] = [];
  for (const t of [...tags].sort((a, b) => a.tag.split("/").length - b.tag.split("/").length)) {
    const i = t.tag.lastIndexOf("/");
    const node: Node = { path: t.tag, name: t.tag.slice(i + 1), count: t.count, kids: [] };
    byPath.set(t.tag, node);
    (i < 0 ? roots : (byPath.get(t.tag.slice(0, i))?.kids ?? roots)).push(node);
  }
  const sort = (ns: Node[]) => {
    ns.sort((a, b) => collator.compare(a.name, b.name));
    ns.forEach((n) => sort(n.kids));
  };
  sort(roots);
  return roots;
}

function RenameDialog(props: { tag: string; count: number; onClose: () => void }) {
  const [name, setName] = useState(props.tag);
  const [error, setError] = useState<string | null>(null);
  const m = useNoteMutations();
  const toast = useToast();
  const submit = (e: FormEvent) => {
    e.preventDefault();
    m.renameTag.mutate(
      { from: props.tag, to: name },
      {
        onSuccess: (n) => {
          if (n > 0) toast(`已改到 ${n} 条笔记`);
          props.onClose();
        },
        onError: (err) => setError(errorText(err)),
      },
    );
  };
  return (
    <Modal title={`标签改名 · ${props.count} 条笔记`} onClose={props.onClose} onSubmit={submit} error={error} submitLabel="改名">
      <Field label="新名称">
        <TextInput autoFocus value={name} onChange={(e) => setName(e.target.value)} />
      </Field>
    </Modal>
  );
}

const OPEN_KEY = "notes.tags_open";

export function TagTree(props: { tags: { tag: string; count: number }[]; active: string | null; onSelect: (tag: string) => void }) {
  const [open, setOpen] = useSetting<string[]>(OPEN_KEY, [], isStringArray);
  const [renaming, setRenaming] = useState<Node | null>(null);
  const roots = buildTree(props.tags);
  const isOpen = (n: Node) => open.includes(n.path) || (!!props.active && props.active.startsWith(`${n.path}/`));
  const toggle = (n: Node) => setOpen(open.includes(n.path) ? open.filter((p) => p !== n.path) : [...open, n.path]);

  const row = (n: Node, depth: number) => {
    const on = props.active === n.path;
    const expanded = isOpen(n);
    return (
      <div key={n.path}>
        <div className={`group/tag flex items-center rounded-control transition-colors duration-150 ${on ? "bg-accent" : "hover:bg-muted/70"}`}>
          <button
            type="button"
            aria-current={on}
            onClick={() => props.onSelect(n.path)}
            className="flex min-w-0 flex-1 items-center gap-1.5 rounded-control py-1.5 pr-1 text-left text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
            style={{ paddingLeft: 10 + depth * 14 }}
          >
            <Hash size={13} strokeWidth={1.75} className="shrink-0 text-muted-foreground" />
            <span className="min-w-0 flex-1 truncate">{n.name}</span>
            <span className="shrink-0 text-xs text-muted-foreground tabular-nums">{n.count}</span>
          </button>
          <button
            type="button"
            aria-label={`给「${n.path}」改名`}
            title="改名"
            onClick={() => setRenaming(n)}
            className="shrink-0 rounded p-0.5 text-muted-foreground opacity-0 outline-none group-hover/tag:opacity-100 hover:bg-muted hover:text-foreground focus-visible:opacity-100 focus-visible:ring-2 focus-visible:ring-ring"
          >
            <Pencil size={12} />
          </button>
          <span className="mr-1.5 flex w-[18px] shrink-0 justify-center">
            {n.kids.length > 0 && (
              <button
                type="button"
                aria-label={expanded ? `收起「${n.path}」` : `展开「${n.path}」`}
                aria-expanded={expanded}
                onClick={() => toggle(n)}
                className="rounded p-0.5 text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
              >
                <ChevronDown size={13} className={`transition-transform duration-150 ${expanded ? "" : "-rotate-90"}`} />
              </button>
            )}
          </span>
        </div>
        {expanded && n.kids.map((k) => row(k, depth + 1))}
      </div>
    );
  };

  return (
    <div className="space-y-0.5">
      {roots.map((n) => row(n, 0))}
      {renaming && <RenameDialog tag={renaming.path} count={renaming.count} onClose={() => setRenaming(null)} />}
    </div>
  );
}
