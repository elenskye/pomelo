// Groups of 素材 and 规则 (Rust catalog.rs `rename_group`): ⋯ → 重命名 (a built-in group only shows the new
// name; one of the user's own moves its records) or 删除这一组 (its records, one undo step). The 规则 page
// says under each group which functions read it, and 「发给模型的说明」 shows each function's system message
// as the real call builds it (Rust model.rs / rater.rs `previews`).
import { useEffect, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { useQuery } from "@tanstack/react-query";
import { Ellipsis, ScrollText, X } from "lucide-react";
import { Dropdown } from "@/shared/Dropdown";
import { Modal } from "@/shared/Modal";
import { useConfirm } from "@/shared/feedback";
import { Button, IconButton, TextInput } from "@/shared/ui";
import { raterApi } from "./raterApi";
import { seekerApi, seekerBatch, type SeekerLang, type SeekerPromptView, type SeekerWorkspace } from "./seekerApi";

type Write = (action: () => Promise<SeekerWorkspace>) => Promise<boolean>;

/** 重命名 / 删除这一组 for a page's groups. */
export function useGroupActions(write: Write, lang: SeekerLang, busy: boolean) {
  const [renaming, setRenaming] = useState<{ kind: "material" | "rule"; category: string; name: string } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [confirm, confirmDialog] = useConfirm();
  const menu = (kind: "material" | "rule", category: string, name: string, ids: string[]) => (
    <Dropdown<string | null>
      plain
      ariaLabel={`「${name}」这一组`}
      value={null}
      options={[{ value: "rename", label: "重命名…" }]}
      actions={ids.length ? [{ label: <span className="text-destructive">删除这一组…</span>, onSelect: () => void remove(name, ids) }] : []}
      onChange={(v) => v === "rename" && (setError(null), setRenaming({ kind, category, name }))}
      renderTrigger={() => <Ellipsis size={14} />}
      triggerClassName="rounded-control p-1 text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
    />
  );
  const remove = async (name: string, ids: string[]) => {
    if (await confirm({ title: `删除「${name}」这一组？`, message: `里面的 ${ids.length} 条一起删除，能撤销；它们的历史里还留着最后一版。`, danger: true, confirmLabel: "删除" })) void write(() => seekerBatch.remove(ids));
  };
  const dialogs = (
    <>
      {renaming && (
        <Modal title="重命名分组" error={error} onClose={() => setRenaming(null)} submitLabel="保存" submitDisabled={busy || !renaming.name.trim()} onSubmit={(e) => { e.preventDefault(); void write(() => seekerApi.groupRename(renaming.kind, renaming.kind === "rule" ? "" : lang, renaming.category, renaming.name)).then((ok) => (ok ? setRenaming(null) : setError("没改成，请重试"))); }}>
          <TextInput autoFocus aria-label="分组名" maxLength={30} value={renaming.name} onChange={(e) => setRenaming({ ...renaming, name: e.target.value })} />
        </Modal>
      )}
      {confirmDialog}
    </>
  );
  return { menu, dialogs };
}

/** Both assistants' functions with the groups they read (the 规则 page's 「发给」 lines and the dialog). */
export function usePrompts(lang: SeekerLang) {
  const seeker = useQuery({ queryKey: ["picky", "seeker", "prompts", lang], queryFn: () => seekerApi.prompts(lang) });
  const rater = useQuery({ queryKey: ["picky", "rater", "prompts"], queryFn: raterApi.prompts });
  return { seeker: seeker.data ?? [], rater: rater.data ?? [], error: seeker.error ?? rater.error, refetch: () => void Promise.all([seeker.refetch(), rater.refetch()]) };
}

/** 「发给：…」: which functions read a group (all of one assistant's in a word). */
export function fedTo(group: string, prompts: ReturnType<typeof usePrompts>): string {
  const part = (name: string, list: SeekerPromptView[]) => {
    const reads = list.filter((p) => p.groups.includes(group));
    if (!reads.length) return [];
    return reads.length === list.length ? [`${name} 的全部功能`] : reads.map((p) => p.label);
  };
  // 「卡片练习 · 追问」「卡片练习 · 点评」 → 「卡片练习（追问、点评）」
  const heads: [string, string[]][] = [];
  for (const label of [...part("Seeker", prompts.seeker), ...part("Rater", prompts.rater)]) {
    const [head, rest] = label.includes(" · ") ? [label.slice(0, label.indexOf(" · ")), label.slice(label.indexOf(" · ") + 3)] : [label, ""];
    const found = heads.find(([h]) => h === head);
    if (found && rest) found[1].push(rest);
    else heads.push([head, rest ? [rest] : []]);
  }
  const all = heads.map(([h, rest]) => (rest.length > 1 ? `${h}（${rest.join("、")}）` : rest.length ? `${h} · ${rest[0]}` : h));
  return all.length ? `发给：${all.join("、")}` : "不发给模型";
}

/** The toolbar's 「发给模型的说明」 and its dialog (one assistant's functions). */
export function PromptsButton(props: { title: string; list: SeekerPromptView[]; label: (group: string) => string; refetch: () => void }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <Button variant="ghost" onClick={() => { props.refetch(); setOpen(true); }}><ScrollText size={14} />发给模型的说明</Button>
      {open && createPortal(<PromptsDialog {...props} onClose={() => setOpen(false)} />, document.body)}
    </>
  );
}

function PromptsDialog(props: { title: string; list: SeekerPromptView[]; label: (group: string) => string; onClose: () => void }) {
  const ref = useRef<HTMLDialogElement>(null);
  const [key, setKey] = useState(props.list[0]?.key ?? "");
  useEffect(() => {
    const d = ref.current;
    if (d && !d.open) d.showModal();
  }, []);
  const shown = props.list.find((p) => p.key === key) ?? props.list[0];
  return (
    <dialog ref={ref} aria-label={props.title} onCancel={(e) => { e.preventDefault(); props.onClose(); }} className="m-auto h-[80vh] w-[880px] max-w-[calc(100vw-48px)] rounded-card border bg-popover p-0 text-foreground shadow-xl backdrop:bg-black/30">
      <div className="flex h-full flex-col">
        <div className="flex items-center gap-2 border-b px-5 py-3">
          <p className="min-w-0 flex-1 truncate text-base font-bold">{props.title}</p>
          <IconButton label="关闭" onClick={props.onClose}><X size={16} /></IconButton>
        </div>
        <div className="flex min-h-0 flex-1">
          <nav aria-label="功能" className="-m-1 w-52 shrink-0 space-y-0.5 overflow-y-auto border-r p-1 px-3 py-3">
            {props.list.map((p) => (
              <button key={p.key} type="button" aria-current={p.key === shown?.key} onClick={() => setKey(p.key)} className={`block w-full truncate rounded-control px-2 py-1.5 text-left text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring ${p.key === shown?.key ? "bg-accent" : "hover:bg-muted/70"}`}>{p.label}</button>
            ))}
          </nav>
          {shown ? (
            <div className="-m-1 flex min-w-0 flex-1 flex-col gap-3 overflow-y-auto p-1 px-5 py-4">
              <p className="text-sm text-muted-foreground">读的规则：{shown.groups.map(props.label).join(" · ")}</p>
              <pre className="selectable whitespace-pre-wrap rounded-control border bg-card p-4 font-sans text-sm leading-relaxed">{shown.system}</pre>
            </div>
          ) : <p className="p-5 text-sm text-muted-foreground">还没读到说明</p>}
        </div>
      </div>
    </dialog>
  );
}

/** A group's head: its name, ＋, ⋯, and (规则) the 「发给」 line under it. */
export function GroupHead(props: { title: string; add?: ReactNode; menu?: ReactNode; note?: string }) {
  return (
    <div>
      <div className="flex h-7 items-center gap-1"><h3 className="text-sm font-bold">{props.title}</h3>{props.add}{props.menu}</div>
      {props.note && <p className="text-xs text-muted-foreground">{props.note}</p>}
    </div>
  );
}
