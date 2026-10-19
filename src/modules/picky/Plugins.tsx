// 插件库 (v0.4 phase 6; beside 管理 at the bottom of the conversation list): the skills (rules in
// Rust skills/). Left the list, right the chosen one: what it does and sends, 启用 / 小拾自己决定用, its
// keys (keychain only: shown as 已保存, never read back). 小工具 get their own group when one exists.
// Also here: the skills a draft asks for (chips above the input, the 联网 button), sent with it.
import { useEffect, useRef, useState, type FormEvent } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { X } from "lucide-react";
import { errorText, useToast } from "@/shared/feedback";
import { Button, IconButton, Switch, TextInput } from "@/shared/ui";
import { SKILLS_KEY, pickyApi, useSkills, type Skill, type SkillKey } from "./api";

export function PluginsDialog(props: { onClose: () => void }) {
  const ref = useRef<HTMLDialogElement>(null);
  const skills = useSkills().data ?? [];
  const [chosen, setChosen] = useState<string | null>(null);
  useEffect(() => {
    const d = ref.current;
    if (d && !d.open) d.showModal();
  }, []);
  const shown = skills.find((s) => s.key === chosen) ?? skills[0];
  return (
    <dialog
      ref={ref}
      aria-label="插件库"
      onCancel={(e) => {
        e.preventDefault();
        props.onClose();
      }}
      className="m-auto flex h-[70vh] max-h-[640px] w-[760px] max-w-[92vw] flex-col rounded-card border bg-popover p-0 text-foreground shadow-xl backdrop:bg-black/30"
    >
      <div className="flex items-center gap-4 border-b px-5 py-3">
        <p className="text-base font-bold">插件库</p>
        <div className="flex-1" />
        <IconButton label="关闭" onClick={props.onClose}>
          <X size={16} />
        </IconButton>
      </div>
      <div className="flex min-h-0 flex-1">
        <nav aria-label="插件" className="-m-1 w-52 shrink-0 overflow-y-auto border-r p-1 px-3 py-3">
          <p className="px-2 pb-1 text-xs text-muted-foreground">技能</p>
          {skills.map((s) => (
            <button
              key={s.key}
              type="button"
              onClick={() => setChosen(s.key)}
              aria-current={s.key === shown?.key}
              className={`flex w-full items-center gap-2 rounded-control px-2 py-1.5 text-left text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring ${s.key === shown?.key ? "bg-accent" : "hover:bg-muted/70"} ${s.enabled ? "" : "text-muted-foreground"}`}
            >
              <span className="min-w-0 flex-1 truncate">{s.name}</span>
              {s.enabled && !s.ready && <span className="size-1.5 shrink-0 rounded-full bg-primary" aria-label="还缺 Key" />}
            </button>
          ))}
        </nav>
        <div className="-m-1 min-w-0 flex-1 overflow-y-auto p-1 px-6 py-5">{shown && <SkillPane skill={shown} />}</div>
      </div>
    </dialog>
  );
}

function SkillPane(props: { skill: Skill }) {
  const s = props.skill;
  const qc = useQueryClient();
  const toast = useToast();
  const set = (p: { enabled?: boolean; auto?: boolean }) =>
    void pickyApi.skillSet(s.key, p.enabled ?? null, p.auto ?? null).then(
      (list) => qc.setQueryData(SKILLS_KEY, list),
      (e) => toast(errorText(e), "error"),
    );
  return (
    <div className="flex max-w-lg flex-col gap-4 text-sm">
      <div className="flex items-baseline gap-2">
        <p className="text-base font-bold">{s.name}</p>
        <span className="rounded-control bg-muted px-1.5 text-xs text-muted-foreground">{s.command}</span>
      </div>
      <p className="leading-relaxed">{s.summary}</p>
      <p className="leading-relaxed text-muted-foreground">{s.detail}</p>
      <div className="flex flex-col gap-2.5 border-t pt-4">
        <label className="flex items-center justify-between gap-4">
          <span>启用</span>
          <Switch label="启用" checked={s.enabled} onChange={(v) => set({ enabled: v })} />
        </label>
        <label className={`flex items-center justify-between gap-4 ${s.enabled ? "" : "opacity-50"}`}>
          <span>小拾自己决定用</span>
          <Switch label="小拾自己决定用" checked={s.auto} onChange={(v) => s.enabled && set({ auto: v })} />
        </label>
      </div>
      {s.usesContact && <ContactField value={s.contact} />}
      {s.keys.map((k) => (
        <KeyField key={k.account} spec={k} />
      ))}
    </div>
  );
}

/** The contact 书单 / 影单检索 send to Wikidata and Open Library (one for both). */
function ContactField(props: { value: string | null }) {
  const qc = useQueryClient();
  const toast = useToast();
  const [value, setValue] = useState(props.value ?? "");
  useEffect(() => setValue(props.value ?? ""), [props.value]);
  const save = async (e: FormEvent) => {
    e.preventDefault();
    try {
      qc.setQueryData(SKILLS_KEY, await pickyApi.skillContactSet(value));
      toast(value.trim() ? "已保存联系邮箱" : "已清掉联系邮箱");
    } catch (err) {
      toast(errorText(err), "error");
    }
  };
  return (
    <div className="flex flex-col gap-1.5 border-t pt-4">
      <p>
        联系邮箱<span className="text-muted-foreground">（书单检索、影单检索共用）</span>
      </p>
      <p className="text-muted-foreground">只发给 Wikidata 和 Open Library。填了 Wikidata 每分钟给 200 次请求，不填只有 10 次，几本一起查时会慢、可能查不全。</p>
      <form onSubmit={save} className="flex items-center gap-2">
        <TextInput type="email" autoComplete="off" aria-label="联系邮箱" value={value} onChange={(e) => setValue(e.target.value)} className="min-w-0 flex-1" />
        <Button type="submit" variant="primary" disabled={value.trim() === (props.value ?? "")}>
          保存
        </Button>
      </form>
    </div>
  );
}

function KeyField(props: { spec: SkillKey }) {
  const k = props.spec;
  const qc = useQueryClient();
  const toast = useToast();
  const [editing, setEditing] = useState(false);
  const [value, setValue] = useState("");
  const save = async (e: FormEvent) => {
    e.preventDefault();
    try {
      qc.setQueryData(SKILLS_KEY, await pickyApi.skillKeySave(k.account, value));
      setValue("");
      setEditing(false);
      toast(`已保存 ${k.label}`);
    } catch (err) {
      toast(errorText(err), "error");
    }
  };
  const remove = () =>
    void pickyApi.skillKeyRemove(k.account).then(
      (list) => qc.setQueryData(SKILLS_KEY, list),
      (e) => toast(errorText(e), "error"),
    );
  return (
    <div className="flex flex-col gap-1.5 border-t pt-4">
      <p>
        {k.label}
        {!k.required && <span className="text-muted-foreground">（可不填）</span>}
      </p>
      {editing || !k.saved ? (
        <form onSubmit={save} className="flex items-center gap-2">
          <TextInput type="password" autoComplete="off" aria-label={k.label} value={value} onChange={(e) => setValue(e.target.value)} className="min-w-0 flex-1 font-mono" />
          <Button type="submit" variant="primary" disabled={!value.trim()}>
            保存
          </Button>
          {editing && <Button onClick={() => (setEditing(false), setValue(""))}>取消</Button>}
        </form>
      ) : (
        <div className="flex items-center gap-2">
          <span className="flex-1 text-muted-foreground">已保存在钥匙串里</span>
          <Button onClick={() => setEditing(true)}>更新</Button>
          <Button variant="ghost-danger" onClick={remove}>
            移除
          </Button>
        </div>
      )}
    </div>
  );
}

// ---------- asking for skills (the input of the 小拾 page and the 小窗) ----------

/** The skills each draft asks for (「/名称」, 联网): only for its next question. */
export function useAskedSkills(draftKey: string) {
  const skills = useSkills().data ?? [];
  const toast = useToast();
  const [asked, setAsked] = useState<Record<string, string[]>>({});
  const now = asked[draftKey] ?? [];
  const usable = skills.filter((s) => s.enabled && s.ready);
  const toggle = (key: string) => {
    const s = skills.find((x) => x.key === key);
    if (!s?.enabled) return;
    if (!s.ready) return toast(`${s.name}还没填 ${s.keys.filter((k) => k.required).map((k) => k.label).join("、")}（插件库）`, "error");
    setAsked((m) => ({ ...m, [draftKey]: now.includes(key) ? now.filter((k) => k !== key) : [...now, key] }));
  };
  const clear = (key: string) => setAsked(({ [key]: _, ...rest }) => rest);
  return { skills, asked: now, usable: usable.map((s) => ({ key: s.key, name: s.name })), toggle, clear };
}

/** Above the input: the skills the next question asks for. */
export function SkillChips(props: { skills: Skill[]; asked: string[]; onRemove: (key: string) => void }) {
  return (
    <>
      {props.asked.map((k) => {
        const s = props.skills.find((x) => x.key === k);
        return (
          <span key={k} className="inline-flex h-6 items-center gap-1 rounded-full bg-accent pr-1 pl-2.5 text-xs text-accent-foreground">
            {s?.command ?? k}
            <button type="button" aria-label={`不用${s?.name ?? k}`} onClick={() => props.onRemove(k)} className="rounded-full p-0.5 outline-none hover:bg-black/5 focus-visible:ring-2 focus-visible:ring-ring">
              <X size={11} />
            </button>
          </span>
        );
      })}
    </>
  );
}
