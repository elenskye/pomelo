// Side panel for one application. 状态: pick which statuses it goes through (chips), then check them
// off on the timeline below — checking fills today's date, which stays editable. Rust derives the
// current status (last checked; none = 想投) and 投递日期 (first checked day).
import { useEffect, useMemo, useRef, useState, type FormEvent } from "react";
import { Check, ExternalLink, FolderOpen, Settings2 } from "lucide-react";
import { AssistAction, Polish, QuickFill, assistApi, assistBusyText, useAssistOn, useFilled, type JobFill, type QuickFillResult } from "@/modules/picky/Assist";
import { usePickyStatus } from "@/modules/picky/api";
import { homeDir } from "@tauri-apps/api/path";
import { open } from "@tauri-apps/plugin-dialog";
import { DateInput } from "@/shared/DateInput";
import { localToday } from "@/shared/dates";
import { Dropdown } from "@/shared/Dropdown";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { api as ipc } from "@/shared/ipc";
import { useKinds, UNCATEGORIZED } from "@/shared/kinds/api";
import { SidePanel } from "@/shared/panel";
import { Button, Field, IconButton, TextArea, TextInput } from "@/shared/ui";
import { CHANNELS, jobLabel, useJobMutations, useStatuses, type Job, type Status } from "./api";
import { JobSeekerExports } from "./SeekerExports";

type DraftStep = { statusId: number; done: boolean; doneOn: string };

type Draft = {
  company: string;
  position: string;
  location: string;
  kindId: number | null;
  link: string;
  resume: string;
  note: string;
  steps: DraftStep[];
};

const SEEKER = "Documents/Seeker";

/** Path / URL input with 打开 (and 选择… for files, starting in Seeker). Paths under home are kept as ~/…. */
function OpenableInput(props: { label: string; value: string; onChange: (v: string) => void; file?: boolean }) {
  const toast = useToast();
  const pick = async () => {
    const home = (await homeDir()).replace(/\/$/, "");
    const path = await open({ multiple: false, directory: false, defaultPath: `${home}/${SEEKER}` });
    if (typeof path === "string") props.onChange(path.startsWith(`${home}/`) ? `~/${path.slice(home.length + 1)}` : path);
  };
  return (
    <Field label={props.label}>
      <div className="flex items-center gap-1">
        <TextInput aria-label={props.label} value={props.value} onChange={(e) => props.onChange(e.target.value)} maxLength={1000} />
        {props.file && (
          <IconButton label={`选择${props.label}文件`} onClick={() => void pick().catch((e) => toast(errorText(e), "error"))}>
            <FolderOpen size={14} />
          </IconButton>
        )}
        <IconButton label={`打开${props.label}`} disabled={!props.value.trim()} onClick={() => void ipc.openLink(props.value).catch((e) => toast(errorText(e), "error"))}>
          <ExternalLink size={14} />
        </IconButton>
      </div>
    </Field>
  );
}

/** Chips for every non-准备 status (which steps this application has), then those steps as a timeline. */
function Steps(props: { statuses: Status[]; value: DraftStep[]; onChange: (v: DraftStep[]) => void; onManage: () => void }) {
  const choosable = props.statuses.filter((s) => s.stage !== "wish");
  const name = new Map(props.statuses.map((s) => [s.id, s.name]));
  const has = (id: number) => props.value.some((s) => s.statusId === id);
  const toggle = (id: number) => {
    const next = has(id) ? props.value.filter((s) => s.statusId !== id) : [...props.value, { statusId: id, done: false, doneOn: "" }];
    const order = (sid: number) => props.statuses.findIndex((s) => s.id === sid);
    props.onChange(next.sort((a, b) => order(a.statusId) - order(b.statusId)));
  };
  const set = (id: number, p: Partial<DraftStep>) => props.onChange(props.value.map((s) => (s.statusId === id ? { ...s, ...p } : s)));
  return (
    <>
      <div className="flex flex-wrap items-center gap-1.5">
        {choosable.map((s) => {
          const on = has(s.id);
          return (
            <button
              key={s.id}
              type="button"
              aria-pressed={on}
              onClick={() => toggle(s.id)}
              className={`rounded-full border px-2.5 py-0.5 text-xs transition-colors duration-150 outline-none focus-visible:ring-2 focus-visible:ring-ring ${
                on ? "border-transparent bg-accent text-accent-foreground" : "text-muted-foreground hover:bg-muted"
              }`}
            >
              {s.name}
            </button>
          );
        })}
        <IconButton label="管理状态" onClick={props.onManage}>
          <Settings2 size={14} />
        </IconButton>
      </div>
      {props.value.length > 0 && (
        <ol className="relative mt-2">
          {props.value.length > 1 && <span aria-hidden className="absolute top-5 bottom-5 left-[9px] w-px bg-border" />}
          {props.value.map((s) => {
            const label = name.get(s.statusId) ?? "";
            return (
              <li key={s.statusId} className="relative flex h-10 items-center gap-3">
                <button
                  type="button"
                  role="checkbox"
                  aria-checked={s.done}
                  aria-label={label}
                  onClick={() => set(s.statusId, s.done ? { done: false, doneOn: "" } : { done: true, doneOn: localToday() })}
                  className={`grid size-[19px] shrink-0 place-items-center rounded-[5px] border transition-colors duration-150 outline-none focus-visible:ring-2 focus-visible:ring-ring ${
                    s.done ? "border-primary bg-primary text-primary-foreground" : "bg-card hover:border-foreground/40"
                  }`}
                >
                  {s.done && <Check size={13} strokeWidth={3} />}
                </button>
                <span className={`min-w-0 flex-1 truncate text-sm ${s.done ? "" : "text-muted-foreground"}`}>{label}</span>
                {s.done && <DateInput clearable ariaLabel={`${label}的日期`} value={s.doneOn} onChange={(v) => set(s.statusId, { doneOn: v })} />}
              </li>
            );
          })}
        </ol>
      )}
    </>
  );
}

/** 「贴 JD」 (opened from the 公司 label row): 小拾 finds 公司、岗位、地点 in it; the panel fills the empty
 * ones and keeps the JD in 备注. */
function JdFill(props: { onFill: (f: JobFill, jd: string) => void; onClose: () => void }) {
  const status = usePickyStatus().data;
  const toast = useToast();
  const [jd, setJd] = useState("");
  const [busy, setBusy] = useState(false);
  const run = async () => {
    setBusy(true);
    try {
      props.onFill(await assistApi.fillJob(jd), jd);
      props.onClose();
    } catch (e) {
      toast(errorText(e), "error");
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="space-y-2 rounded-card border bg-card p-2.5 shadow-sm">
      <TextArea aria-label="JD" rows={6} autoFocus value={jd} onChange={(e) => setJd(e.target.value)} maxLength={20000} />
      <div className="flex gap-2">
        <Button variant="primary" disabled={!jd.trim() || busy} onClick={() => void run()}>
          {busy ? assistBusyText(status, "正在读 JD…") : "补全"}
        </Button>
        <Button variant="ghost" onClick={props.onClose}>
          取消
        </Button>
      </div>
    </div>
  );
}

export function JobPanel(props: {
  job: Job | null;
  onClose: () => void;
  onDone: () => void;
  onDirtyChange: (dirty: boolean) => void;
  onManageStatuses: () => void;
  onManageChannels: () => void;
}) {
  const j = props.job;
  const { data: statuses = [] } = useStatuses();
  const { data: channels = [] } = useKinds(CHANNELS);
  // a new application starts with every status that isn't 准备 / 结束 (as Rust does)
  const defaults = useMemo<DraftStep[]>(
    () => statuses.filter((s) => s.stage !== "wish" && s.stage !== "closed").map((s) => ({ statusId: s.id, done: false, doneOn: "" })),
    [statuses],
  );
  const initial = useMemo<Draft>(
    () => ({
      company: j?.company ?? "",
      position: j?.position ?? "",
      location: j?.location ?? "",
      kindId: j?.kindId ?? null,
      link: j?.link ?? "",
      resume: j?.resume ?? "",
      note: j?.note ?? "",
      steps: j ? j.steps.map((s) => ({ statusId: s.statusId, done: s.done, doneOn: s.doneOn ?? "" })) : defaults,
    }),
    [j, defaults],
  );
  const [d, setD] = useState(initial);
  // statuses may load after the panel opens: give a still-untouched new application its default steps
  const seeded = useRef(j !== null || defaults.length > 0);
  useEffect(() => {
    if (!seeded.current && defaults.length) {
      seeded.current = true;
      setD((x) => ({ ...x, steps: defaults }));
    }
  }, [defaults]);
  const dirty = JSON.stringify(d) !== JSON.stringify(initial);
  useEffect(() => {
    props.onDirtyChange(dirty);
  }, [dirty]); // eslint-disable-line react-hooks/exhaustive-deps
  const filled = useFilled<keyof Draft>();
  const set = <K extends keyof Draft>(k: K, v: Draft[K]) => {
    filled.clear(k);
    setD((x) => ({ ...x, [k]: v }));
  };
  /** 小拾 read a JD: empty fields get what it found, the JD itself goes into 备注. */
  const fromJd = (f: JobFill, jd: string) => {
    const next = { ...d };
    const keys: (keyof Draft)[] = ["note"];
    for (const k of ["company", "position", "location"] as const) {
      const v = f[k];
      if (v && !d[k].trim()) {
        next[k] = v;
        keys.push(k);
      }
    }
    next.note = d.note.trim() ? `${d.note.trimEnd()}\n\n${jd.trim()}` : jd.trim();
    setD(next);
    filled.mark(keys);
  };
  // 一句话填好 (new applications): 公司、岗位、地点、渠道, and the step it reached (checked on its day)
  const assistOn = useAssistOn();
  const quick = !j && assistOn;
  const [jdOpen, setJdOpen] = useState(false);
  const fill = (f: QuickFillResult) => {
    const next = { ...d };
    const keys: (keyof Draft)[] = [];
    for (const k of ["company", "position", "location"] as const) {
      const v = f[k];
      if (v) (next[k] = v), keys.push(k);
    }
    if (f.kindId != null) (next.kindId = f.kindId), keys.push("kindId");
    if (f.statusId != null) {
      const step = { statusId: f.statusId, done: true, doneOn: f.day ?? localToday() };
      const order = (id: number) => statuses.findIndex((x) => x.id === id);
      const steps = d.steps.some((x) => x.statusId === f.statusId) ? d.steps.map((x) => (x.statusId === f.statusId ? step : x)) : [...d.steps, step];
      next.steps = steps.sort((a, b) => order(a.statusId) - order(b.statusId));
      keys.push("steps");
    }
    setD(next);
    filled.mark(keys);
  };
  const [error, setError] = useState<string | null>(null);
  const m = useJobMutations();
  const toast = useToast();
  const [confirm, confirmDialog] = useConfirm();

  const save = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    const t = (s: string) => s.trim() || null;
    const patch = {
      company: d.company,
      position: t(d.position),
      location: t(d.location),
      kindId: d.kindId,
      link: t(d.link),
      resume: t(d.resume),
      note: t(d.note),
      steps: d.steps.map((s) => ({ statusId: s.statusId, done: s.done, doneOn: s.done ? s.doneOn || null : null })),
    };
    try {
      if (j) {
        if (dirty) await m.update.mutateAsync({ id: j.id, patch });
        toast("已保存");
      } else {
        await m.create.mutateAsync(patch);
        toast(`已添加「${jobLabel({ company: d.company.trim(), position: t(d.position) })}」`);
      }
      props.onDone();
    } catch (err) {
      setError(errorText(err));
    }
  };
  const del = async () => {
    if (!j || !(await confirm({ title: `删除「${jobLabel(j)}」？`, confirmLabel: "删除", danger: true }))) return;
    m.remove.mutate(j.id, {
      onSuccess: () => {
        toast("已删除");
        props.onDone();
      },
    });
  };

  return (
    <SidePanel
      title={j ? "编辑投递" : "新投递"}
      onClose={props.onClose}
      onSubmit={save}
      onDelete={j ? () => void del() : undefined}
      error={error}
      meta={j?.followedOn ? `${j.followedOn.split("-").join(" / ")} 跟进过` : undefined}
      extra={confirmDialog}
    >
      {quick && <QuickFill module="jobs" onFill={fill} autoFocus />}
      <Field label="公司" action={assistOn && !jdOpen && <AssistAction onClick={() => setJdOpen(true)}>贴 JD</AssistAction>}>
        {jdOpen && <JdFill onFill={fromJd} onClose={() => setJdOpen(false)} />}
        <TextInput autoFocus={!quick} value={d.company} onChange={(e) => set("company", e.target.value)} placeholder="必填" maxLength={100} className={filled.cls("company")} />
      </Field>
      <div className="grid grid-cols-[1fr_8rem] gap-3">
        <Field label="岗位">
          <TextInput value={d.position} onChange={(e) => set("position", e.target.value)} maxLength={100} className={filled.cls("position")} />
        </Field>
        <Field label="地点">
          <TextInput value={d.location} onChange={(e) => set("location", e.target.value)} maxLength={50} className={filled.cls("location")} />
        </Field>
      </div>
      <Field label="渠道">
        <div className={filled.halo("kindId")}>
          <Dropdown
            ariaLabel="渠道"
            value={d.kindId}
            onChange={(v) => set("kindId", v)}
            options={[...channels.map((k) => ({ value: k.id as number | null, label: k.name })), { value: null, label: UNCATEGORIZED }]}
            actions={[{ label: "管理渠道…", onSelect: props.onManageChannels }]}
          />
        </div>
      </Field>
      <Field label="状态">
        <div>
          <div className={filled.halo("steps")}>
            <Steps statuses={statuses} value={d.steps} onChange={(v) => set("steps", v)} onManage={props.onManageStatuses} />
          </div>
        </div>
      </Field>
      <OpenableInput label="JD 链接" value={d.link} onChange={(v) => set("link", v)} />
      <OpenableInput label="简历" file value={d.resume} onChange={(v) => set("resume", v)} />
      {j && <JobSeekerExports job={j} disabled={dirty} />}
      <Field label="备注">
        <Polish module="jobs" value={d.note} onChange={(v) => set("note", v)}>
          <TextArea rows={4} value={d.note} onChange={(e) => set("note", e.target.value)} maxLength={5000} className={filled.cls("note")} />
        </Polish>
      </Field>
    </SidePanel>
  );
}
