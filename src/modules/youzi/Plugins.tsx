// 插件库 (v0.4 phase 6; beside 管理 at the bottom of the conversation list): the skills (rules in
// Rust skills/). Left the list, right the chosen one: what it does and sends, 启用 / 柚子自己决定用, its
// keys (keychain only: shown as 已保存, never read back). 小工具 get their own group when one exists.
// Also here: the skills a draft asks for (chips above the input, the 联网 button), sent with it.
import { Fragment, useEffect, useRef, useState, type FormEvent, type ReactNode } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { X, RefreshCw } from "@/shared/icons";
import { errorText, useToast } from "@/shared/feedback";
import { Button, IconButton, Switch, TextInput } from "@/shared/ui";
import { SKILLS_KEY, youziApi, useSkills, type Skill, type SkillFlowNode, type SkillKey } from "./api";

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
      className="m-auto flex h-[78vh] max-h-[760px] w-[1000px] max-w-[94vw] flex-col rounded-card border bg-popover p-0 text-foreground shadow-xl backdrop:bg-black/30"
    >
      <div className="flex items-center gap-4 border-b px-5 py-3">
        <p className="text-base font-bold">插件库</p>
        <div className="flex-1" />
        <IconButton label="关闭" onClick={props.onClose}>
          <X size={16} />
        </IconButton>
      </div>
      <div className="flex min-h-0 flex-1 overflow-hidden">
        <nav aria-label="插件" className="-m-1 w-52 shrink-0 overflow-y-auto border-r p-1 px-3 py-3">
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
    void youziApi.skillSet(s.key, p.enabled ?? null, p.auto ?? null).then(
      (list) => qc.setQueryData(SKILLS_KEY, list),
      (e) => toast(errorText(e), "error"),
    );
  return (
    <div className="flex flex-col gap-4 text-sm">
      <div className="flex items-baseline gap-2">
        <p className="text-base font-bold">{s.name}</p>
        <span className="rounded-control bg-muted px-1.5 text-xs text-muted-foreground">{s.command}</span>
      </div>
      <FlowChart flow={s.flow} />
      <div className="flex max-w-lg flex-col gap-4">
        <ul className="list-disc space-y-1 pl-5 leading-relaxed">{s.features.map((f) => <li key={f}>{f}</li>)}</ul>
        <div className="flex flex-col gap-2.5 border-t pt-4">
          <label className="flex items-center justify-between gap-4">
            <span>启用</span>
            <Switch label="启用" checked={s.enabled} onChange={(v) => set({ enabled: v })} />
          </label>
          <label className={`flex items-center justify-between gap-4 ${s.enabled ? "" : "opacity-50"}`}>
            <span>柚子自己决定用</span>
            <Switch label="柚子自己决定用" checked={s.auto} onChange={(v) => s.enabled && set({ auto: v })} />
          </label>
        </div>
        {s.usesContact && <ContactField value={s.contact} />}
        {s.keys.map((k) => (
          <KeyField key={k.account} spec={k} />
        ))}
      </div>
    </div>
  );
}

type Check = Extract<SkillFlowNode, { kind: "check" }>;
/** merge: the check's 是 side goes on at a later step, the other side (`rest`: the steps until then) too,
 * and both meet there; else its 是 side ends (or goes back) and the line goes straight on. */
type Tree = Exclude<SkillFlowNode, Check> | { kind: "fork"; check: Check; merge: boolean; rest: Tree[] };

/** The flow as a tree; `exit`: the step this run flows into (where an outer fork meets). */
function grow(nodes: SkillFlowNode[], exit: string | null): Tree[] {
  const out: Tree[] = [];
  for (let i = 0; i < nodes.length; i++) {
    const n = nodes[i];
    if (n.kind !== "check") {
      out.push(n);
      continue;
    }
    const rest = nodes.slice(i + 1);
    const at = n.join ? rest.findIndex((m) => m.kind === "step" && m.text === n.join) : -1;
    if (at >= 0) {
      out.push({ kind: "fork", check: n, merge: true, rest: grow(rest.slice(0, at), n.join) });
      i += at;
    } else if (n.join && n.join === exit) {
      out.push({ kind: "fork", check: n, merge: true, rest: grow(rest, exit) });
      break;
    } else out.push({ kind: "fork", check: n, merge: false, rest: [] });
  }
  return out;
}

/** A line down (`grow`: as long as its branch). */
const Down = (props: { grow?: boolean; short?: boolean }) => <span aria-hidden className={`block w-px shrink-0 bg-muted-foreground/45 ${props.grow ? "min-h-3 flex-1" : props.short ? "h-2" : "h-3.5"}`} />;
/** A branch's half of the bar it hangs from (top) or meets the other on (bottom): the half toward the middle. */
const Half = (props: { side: "left" | "right"; at: "top" | "bottom" }) => <span aria-hidden className={`absolute h-px bg-muted-foreground/45 ${props.at === "top" ? "top-0" : "bottom-0"} ${props.side === "left" ? "right-0 left-1/2" : "right-1/2 left-0"}`} />;

function Box(props: { tone?: "first" | "last" | "check"; narrow?: boolean; children: ReactNode }) {
  const tone = props.tone === "first" ? "bg-accent text-accent-foreground" : props.tone === "last" ? "border-primary bg-card" : props.tone === "check" ? "bg-muted" : "bg-card";
  return <span className={`block rounded-card border px-3 py-1.5 text-center ${props.narrow ? "max-w-[9rem] text-xs" : "max-w-[15rem] text-sm"} ${tone}`}>{props.children}</span>;
}

/** One side of a fork: hangs from the bar, its label, its steps; `on`: goes on down to where both meet. */
function Branch(props: { side: "left" | "right"; label: string; on: boolean; children?: ReactNode }) {
  return (
    <div className="relative flex flex-col items-center px-2.5">
      <Half side={props.side} at="top" />
      <Down short />
      <span className="py-0.5 text-xs text-muted-foreground">{props.label}</span>
      <Down short />
      {props.children}
      {props.on && (
        <>
          <Down grow />
          <Half side={props.side} at="bottom" />
        </>
      )}
    </div>
  );
}

function Run(props: { items: Tree[]; ends: string[]; narrow?: boolean }) {
  return (
    <div className="flex flex-col items-center">
      {props.items.map((t, i) => (
        <Fragment key={i}>
          {i > 0 && <Down />}
          <Node t={t} ends={props.ends} narrow={props.narrow} />
        </Fragment>
      ))}
    </div>
  );
}

function Node(props: { t: Tree; ends: string[]; narrow?: boolean }) {
  const t = props.t;
  if (t.kind === "step") return <Box narrow={props.narrow} tone={t.text === props.ends[0] ? "first" : t.text === props.ends[1] ? "last" : undefined}>{t.text}</Box>;
  if (t.kind === "again")
    return (
      <span className={`flex items-center gap-1.5 rounded-card border px-3 py-1 text-xs text-muted-foreground ${props.narrow ? "max-w-[9rem]" : "max-w-[15rem]"}`}>
        <RefreshCw size={12} className="shrink-0" aria-hidden />
        {t.text}，回到「{t.back}」
      </span>
    );
  const c = t.check;
  const side = (
    <div className="flex flex-col items-center">
      {c.side.map((s, j) => (
        <Fragment key={j}>
          {j > 0 && <Down />}
          <Box narrow>{s}</Box>
        </Fragment>
      ))}
      {!t.merge && c.join && (
        <>
          <Down short />
          <span className="flex max-w-[9rem] items-center gap-1 text-center text-xs text-muted-foreground">
            <RefreshCw size={12} className="shrink-0" aria-hidden />
            回到「{c.join}」
          </span>
        </>
      )}
    </div>
  );
  return (
    <div className="flex flex-col items-center">
      <Box tone="check" narrow={props.narrow}>{c.ask}</Box>
      <Down />
      {t.merge ? (
        <div className="flex">
          <Branch side="left" label={c.yes} on>{side}</Branch>
          <Branch side="right" label={c.no} on>{t.rest.length > 0 && <Run items={t.rest} ends={props.ends} narrow />}</Branch>
        </div>
      ) : (
        // the line goes straight on: the 是 side to the left, as much room kept on the right
        <div className="grid grid-cols-[1fr_auto_1fr]">
          <Branch side="left" label={c.yes} on={false}>{side}</Branch>
          <div className="relative flex flex-col items-center">
            <Half side="right" at="top" />
            <Down short />
            <span className="px-1 py-0.5 text-xs text-muted-foreground">{c.no}</span>
            <Down grow />
          </div>
          <span />
        </div>
      )}
    </div>
  );
}

/** A flow (a skill's; 年度总结's in 定时) as a tree, in the middle: boxes top to bottom (the input tinted, the result with a 柚红
 * edge, a check filled) joined by solid lines; a check splits into two labelled lines, which meet again
 * where its 是 side goes on, or its 是 side branches off and ends (or says where it goes back to) while
 * the line goes straight on. A loop is a box saying where it goes back to. Wider than the pane: scrolls. */
export function FlowChart(props: { flow: SkillFlowNode[] }) {
  const steps = props.flow.flatMap((n) => (n.kind === "step" ? [n.text] : []));
  return (
    <div aria-label="流程" className="-m-1 overflow-x-auto p-1">
      <div className="mx-auto flex w-max min-w-full justify-center py-1">
        <Run items={grow(props.flow, null)} ends={[steps[0], steps[steps.length - 1]]} />
      </div>
    </div>
  );
}

/** The contact general-book-lookup / general-film-lookup send to Wikidata and Open Library (one for both). */
function ContactField(props: { value: string | null }) {
  const qc = useQueryClient();
  const toast = useToast();
  const [value, setValue] = useState(props.value ?? "");
  useEffect(() => setValue(props.value ?? ""), [props.value]);
  const save = async (e: FormEvent) => {
    e.preventDefault();
    try {
      qc.setQueryData(SKILLS_KEY, await youziApi.skillContactSet(value));
      toast(value.trim() ? "已保存联系邮箱" : "已清掉联系邮箱");
    } catch (err) {
      toast(errorText(err), "error");
    }
  };
  return (
    <div className="flex flex-col gap-1.5 border-t pt-4">
      <p>
        联系邮箱<span className="text-muted-foreground">（general-book-lookup、general-film-lookup 共用）</span>
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
      qc.setQueryData(SKILLS_KEY, await youziApi.skillKeySave(k.account, value));
      setValue("");
      setEditing(false);
      toast(`已保存 ${k.label}`);
    } catch (err) {
      toast(errorText(err), "error");
    }
  };
  const remove = () =>
    void youziApi.skillKeyRemove(k.account).then(
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

// ---------- asking for skills (the input of the 柚子 page and the 小窗) ----------

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
