// Side panel for one application. Rust stamps 投递日期, keeps 「最远到过」 and the linked task.
import { useEffect, useMemo, useState, type FormEvent } from "react";
import { ExternalLink, FolderOpen } from "lucide-react";
import { homeDir } from "@tauri-apps/api/path";
import { open } from "@tauri-apps/plugin-dialog";
import { Dropdown } from "@/shared/Dropdown";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { api as ipc } from "@/shared/ipc";
import { useKinds, UNCATEGORIZED } from "@/shared/kinds/api";
import { SidePanel } from "@/shared/panel";
import { DateInput } from "@/shared/DateInput";
import { Field, IconButton, TextArea, TextInput } from "@/shared/ui";
import { CHANNELS, jobLabel, useJobMutations, useStatuses, type Job } from "./api";

type Draft = {
  company: string;
  position: string;
  location: string;
  statusId: number | null;
  stepOn: string;
  deadline: string;
  appliedOn: string;
  kindId: number | null;
  link: string;
  resume: string;
  coverLetter: string;
  salary: string;
  note: string;
};

const SEEKER = "Documents/Seeker";

function DateField(props: { label: string; value: string; onChange: (v: string) => void }) {
  return (
    <Field label={props.label}>
      <div>
        <DateInput clearable ariaLabel={props.label} value={props.value} onChange={props.onChange} />
      </div>
    </Field>
  );
}

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
  const initial = useMemo<Draft>(
    () => ({
      company: j?.company ?? "",
      position: j?.position ?? "",
      location: j?.location ?? "",
      statusId: j?.statusId ?? null,
      stepOn: j?.stepOn ?? "",
      deadline: j?.deadline ?? "",
      appliedOn: j?.appliedOn ?? "",
      kindId: j?.kindId ?? null,
      link: j?.link ?? "",
      resume: j?.resume ?? "",
      coverLetter: j?.coverLetter ?? "",
      salary: j?.salary ?? "",
      note: j?.note ?? "",
    }),
    [j],
  );
  const [d, setD] = useState(initial);
  // new applications start in the first 准备 status
  useEffect(() => {
    if (d.statusId === null && statuses.length) setD((x) => ({ ...x, statusId: (statuses.find((s) => s.stage === "wish") ?? statuses[0]).id }));
  }, [statuses, d.statusId]);
  const dirty = JSON.stringify(d) !== JSON.stringify(initial) && !(j === null && JSON.stringify({ ...d, statusId: null }) === JSON.stringify(initial));
  useEffect(() => {
    props.onDirtyChange(dirty);
  }, [dirty]); // eslint-disable-line react-hooks/exhaustive-deps
  const set = <K extends keyof Draft>(k: K, v: Draft[K]) => setD((x) => ({ ...x, [k]: v }));
  const [error, setError] = useState<string | null>(null);
  const m = useJobMutations();
  const toast = useToast();
  const [confirm, confirmDialog] = useConfirm();
  const status = statuses.find((s) => s.id === d.statusId);
  const reached = statuses.find((s) => s.id === j?.reachedId);

  const save = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    const t = (s: string) => s.trim() || null;
    const changedStatus = j !== null && d.statusId !== j.statusId;
    const patch = {
      company: d.company,
      position: t(d.position),
      location: t(d.location),
      statusId: d.statusId ?? undefined,
      // a new status with an untouched date: let Rust start the new step without one
      ...(changedStatus && d.stepOn === initial.stepOn ? {} : { stepOn: t(d.stepOn) }),
      deadline: t(d.deadline),
      appliedOn: t(d.appliedOn),
      kindId: d.kindId,
      link: t(d.link),
      resume: t(d.resume),
      coverLetter: t(d.coverLetter),
      salary: t(d.salary),
      note: t(d.note),
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
      meta={[reached && reached.id !== j?.statusId && `最远到过「${reached.name}」`, j?.followedOn && `${j.followedOn.split("-").join(" / ")} 跟进过`].filter(Boolean).join(" · ") || undefined}
      extra={confirmDialog}
    >
      <Field label="公司">
        <TextInput autoFocus value={d.company} onChange={(e) => set("company", e.target.value)} placeholder="必填" maxLength={100} />
      </Field>
      <div className="grid grid-cols-[1fr_8rem] gap-3">
        <Field label="岗位">
          <TextInput value={d.position} onChange={(e) => set("position", e.target.value)} maxLength={100} />
        </Field>
        <Field label="地点">
          <TextInput value={d.location} onChange={(e) => set("location", e.target.value)} maxLength={50} />
        </Field>
      </div>
      <Field label="状态">
        <div>
          <Dropdown
            ariaLabel="状态"
            value={d.statusId}
            onChange={(v) => set("statusId", v)}
            options={statuses.map((s) => ({ value: s.id as number | null, label: s.name }))}
            actions={[{ label: "管理状态…", onSelect: props.onManageStatuses }]}
          />
        </div>
      </Field>
      {status?.stage === "active" && <DateField label={`${status.name}哪天`} value={d.stepOn} onChange={(v) => set("stepOn", v)} />}
      <div className="flex gap-4">
        <DateField label="截止日期" value={d.deadline} onChange={(v) => set("deadline", v)} />
        <DateField label="投递日期" value={d.appliedOn} onChange={(v) => set("appliedOn", v)} />
      </div>
      <div className="grid grid-cols-[1fr_8rem] gap-3">
        <Field label="渠道">
          <div>
            <Dropdown
              ariaLabel="渠道"
              value={d.kindId}
              onChange={(v) => set("kindId", v)}
              options={[...channels.map((k) => ({ value: k.id as number | null, label: k.name })), { value: null, label: UNCATEGORIZED }]}
              actions={[{ label: "管理渠道…", onSelect: props.onManageChannels }]}
            />
          </div>
        </Field>
        <Field label="薪资">
          <TextInput value={d.salary} onChange={(e) => set("salary", e.target.value)} maxLength={50} />
        </Field>
      </div>
      <OpenableInput label="JD 链接" value={d.link} onChange={(v) => set("link", v)} />
      <OpenableInput label="简历" file value={d.resume} onChange={(v) => set("resume", v)} />
      <OpenableInput label="求职信" file value={d.coverLetter} onChange={(v) => set("coverLetter", v)} />
      <Field label="备注">
        <TextArea rows={4} value={d.note} onChange={(e) => set("note", e.target.value)} maxLength={5000} />
      </Field>
    </SidePanel>
  );
}
