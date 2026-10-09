// Left-column tags, built from full paths (#a/b/c; siblings in pinyin order): pinned first-level tags
// first (pin order, each with its sub-tags), then the rest of the tree. Each row: icon (click = change
// it), name, count, and a ⋯ menu — 置顶 (first level only) / 换图标 / 改名 (rename rewrites the notes;
// Rust moves the tag's settings along). Fold state is a UI setting; the active tag's ancestors always show.
import { useState, type FormEvent } from "react";
import { ChevronDown, Ellipsis, Hash } from "lucide-react";
import { TAG_ICONS, TagIcon } from "./tagIcons";
import { Dropdown } from "@/shared/Dropdown";
import { errorText, useToast } from "@/shared/feedback";
import { Modal } from "@/shared/Modal";
import { isStringArray, useSetting } from "@/shared/settings";
import { Button, Field, TextInput } from "@/shared/ui";
import { useNoteMutations, type TagCount } from "./api";

type Node = { path: string; name: string; count: number; icon: string | null; kids: Node[] };

const collator = new Intl.Collator("zh-Hans-CN-u-co-pinyin", { numeric: true });

export function buildTree(tags: TagCount[]): Node[] {
  const byPath = new Map<string, Node>();
  const roots: Node[] = [];
  for (const t of [...tags].sort((a, b) => a.tag.split("/").length - b.tag.split("/").length)) {
    const i = t.tag.lastIndexOf("/");
    const node: Node = { path: t.tag, name: t.tag.slice(i + 1), count: t.count, icon: t.icon, kids: [] };
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

function IconDialog(props: { tag: string; icon: string | null; onClose: () => void }) {
  const [custom, setCustom] = useState(props.icon && !props.icon.startsWith("i:") ? props.icon : "");
  const m = useNoteMutations();
  const pick = (icon: string | null) => m.setTag.mutate({ tag: props.tag, patch: { icon } }, { onSuccess: props.onClose });
  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (custom.trim()) pick(custom.trim());
  };
  return (
    <Modal title={`#${props.tag} 的图标`} onClose={props.onClose} onSubmit={submit} error={null} submitLabel="用这个 emoji">
      <div className="grid grid-cols-8 gap-1">
        {TAG_ICONS.map(([key, Icon]) => {
          const on = props.icon === `i:${key}`;
          return (
            <button
              key={key}
              type="button"
              aria-label={key}
              aria-pressed={on}
              onClick={() => pick(`i:${key}`)}
              className={`grid h-9 place-items-center rounded-control outline-none transition-colors duration-150 hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring ${on ? "bg-accent" : ""}`}
            >
              <Icon size={17} strokeWidth={1.75} style={{ color: "var(--module-notes)" }} />
            </button>
          );
        })}
      </div>
      <div className="flex items-end gap-2">
        <Field label="其他 emoji">
          <TextInput value={custom} onChange={(e) => setCustom(e.target.value)} maxLength={16} className="w-28 text-center text-lg" />
        </Field>
        {props.icon && (
          <Button variant="ghost" onClick={() => pick(null)}>
            <Hash size={14} /> 不用图标
          </Button>
        )}
      </div>
    </Modal>
  );
}

const OPEN_KEY = "notes.tags_open";

export function TagTree(props: { tags: TagCount[]; pinned: string[]; active: string | null; onSelect: (tag: string) => void }) {
  const [open, setOpen] = useSetting<string[]>(OPEN_KEY, [], isStringArray);
  const [renaming, setRenaming] = useState<TagCount | null>(null);
  const [iconFor, setIconFor] = useState<TagCount | null>(null);
  const m = useNoteMutations();
  const roots = buildTree(props.tags);
  const byPath = new Map(props.tags.map((t) => [t.tag, t]));
  const isOpen = (n: Node) => open.includes(n.path) || (!!props.active && props.active.startsWith(`${n.path}/`));
  const toggle = (n: Node) => setOpen(open.includes(n.path) ? open.filter((p) => p !== n.path) : [...open, n.path]);

  const row = (t: TagCount, label: string, depth: number, kids?: { expanded: boolean; onToggle: () => void }) => {
    const on = props.active === t.tag;
    const top = !t.tag.includes("/");
    const pinned = props.pinned.includes(t.tag);
    return (
      <div
        className={`group/tag flex items-center rounded-control transition-colors duration-150 ${on ? "bg-accent" : "hover:bg-muted/70"}`}
        style={{ paddingLeft: 6 + depth * 14 }}
      >
        <button
          type="button"
          aria-label={`换「${t.tag}」的图标`}
          onClick={() => setIconFor(t)}
          className="grid size-5 shrink-0 place-items-center rounded outline-none hover:bg-black/5 focus-visible:ring-2 focus-visible:ring-ring dark:hover:bg-white/10"
        >
          <TagIcon icon={t.icon} />
        </button>
        <button
          type="button"
          aria-current={on}
          onClick={() => props.onSelect(t.tag)}
          className="flex min-w-0 flex-1 items-center gap-1.5 rounded-control py-1.5 pr-1 pl-2 text-left text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
        >
          <span className="min-w-0 flex-1 truncate">{label}</span>
          <span className="shrink-0 text-xs text-muted-foreground tabular-nums">{t.count}</span>
        </button>
        <Dropdown<string | null>
          plain
          ariaLabel={`「${t.tag}」的更多操作`}
          value={null}
          onChange={(a) => {
            if (a === "pin") m.setTag.mutate({ tag: t.tag, patch: { pinned: !pinned } });
            if (a === "icon") setIconFor(t);
            if (a === "rename") setRenaming(t);
          }}
          align="end"
          options={[
            ...(top ? [{ value: "pin" as string | null, label: pinned ? "取消置顶" : "置顶" }] : []),
            { value: "icon", label: "换图标…" },
            { value: "rename", label: "改名…" },
          ]}
          renderTrigger={() => <Ellipsis size={13} />}
          triggerClassName="shrink-0 rounded p-0.5 text-muted-foreground opacity-0 outline-none group-hover/tag:opacity-100 hover:bg-muted hover:text-foreground focus-visible:opacity-100 focus-visible:ring-2 focus-visible:ring-ring aria-expanded:opacity-100"
        />
        <span className="mr-1.5 flex w-[18px] shrink-0 justify-center">
          {kids && (
            <button
              type="button"
              aria-label={kids.expanded ? `收起「${t.tag}」` : `展开「${t.tag}」`}
              aria-expanded={kids.expanded}
              onClick={kids.onToggle}
              className="rounded p-0.5 text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
            >
              <ChevronDown size={13} className={`transition-transform duration-150 ${kids.expanded ? "" : "-rotate-90"}`} />
            </button>
          )}
        </span>
      </div>
    );
  };

  const node = (n: Node, depth: number) => {
    const expanded = isOpen(n);
    return (
      <div key={n.path}>
        {row(byPath.get(n.path) ?? { tag: n.path, count: n.count, icon: n.icon }, n.name, depth, n.kids.length > 0 ? { expanded, onToggle: () => toggle(n) } : undefined)}
        {expanded && n.kids.map((k) => node(k, depth + 1))}
      </div>
    );
  };

  // pinned first-level tags (with their sub-tags) go on top and aren't repeated in the tree
  const pinnedRoots = props.pinned.map((p) => roots.find((r) => r.path === p)).filter((n) => n !== undefined);
  const rest = roots.filter((r) => !props.pinned.includes(r.path));
  return (
    <>
      {pinnedRoots.length > 0 && (
        <>
          <div className="px-2.5 pt-4 pb-1 text-xs text-muted-foreground">置顶</div>
          <div className="space-y-0.5">{pinnedRoots.map((n) => node(n, 0))}</div>
        </>
      )}
      {rest.length > 0 && (
        <>
          <div className="px-2.5 pt-4 pb-1 text-xs text-muted-foreground">标签</div>
          <div className="space-y-0.5">{rest.map((n) => node(n, 0))}</div>
        </>
      )}
      {renaming && <RenameDialog tag={renaming.tag} count={renaming.count} onClose={() => setRenaming(null)} />}
      {iconFor && <IconDialog tag={iconFor.tag} icon={iconFor.icon} onClose={() => setIconFor(null)} />}
    </>
  );
}
