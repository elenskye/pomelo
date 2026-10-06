// Side panel for one password: 名称 · 链接 · 用户名 · 账号 · 密码 (show / 生成) · 备注. Editing loads the
// full entry from Rust; nothing is cached after the panel closes (gcTime 0).
import { useEffect, useMemo, useState, type FormEvent } from "react";
import { Eye, EyeOff, ExternalLink, WandSparkles } from "lucide-react";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { api as ipc } from "@/shared/ipc";
import { SidePanel } from "@/shared/panel";
import { Field, IconButton, TextArea, TextInput } from "@/shared/ui";
import { useVaultEntry, useVaultMutations, vaultApi, type Entry, type Fields } from "./api";

const EMPTY = { name: "", link: "", username: "", account: "", password: "", note: "" };
type Draft = typeof EMPTY;

const pad = (n: number) => String(n).padStart(2, "0");
const day = (stamp: string) => {
  const d = new Date(stamp);
  return `${d.getFullYear()} / ${pad(d.getMonth() + 1)} / ${pad(d.getDate())}`;
};

export function EntryPanel(props: { id: number | null; onClose: () => void; onDone: () => void; onDirtyChange: (dirty: boolean) => void }) {
  const entryQ = useVaultEntry(props.id);
  if (props.id !== null && !entryQ.data) return null;
  return <Form {...props} entry={entryQ.data ?? null} />;
}

function Form(props: { entry: Entry | null; onClose: () => void; onDone: () => void; onDirtyChange: (dirty: boolean) => void }) {
  const e = props.entry;
  const initial = useMemo<Draft>(
    () => (e ? { name: e.name, link: e.link ?? "", username: e.username ?? "", account: e.account ?? "", password: e.password ?? "", note: e.note ?? "" } : EMPTY),
    [e],
  );
  const [d, setD] = useState(initial);
  const [show, setShow] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const m = useVaultMutations();
  const toast = useToast();
  const [confirm, confirmDialog] = useConfirm();
  const dirty = JSON.stringify(d) !== JSON.stringify(initial);
  useEffect(() => {
    props.onDirtyChange(dirty);
  }, [dirty]); // eslint-disable-line react-hooks/exhaustive-deps
  const set = (k: keyof Draft, v: string) => setD((x) => ({ ...x, [k]: v }));

  const save = async (ev: FormEvent) => {
    ev.preventDefault();
    setError(null);
    const t = (s: string) => s.trim() || null;
    const fields: Fields = { name: d.name, link: t(d.link), username: t(d.username), account: t(d.account), password: d.password || null, note: t(d.note) };
    try {
      if (e) {
        if (dirty) await m.update.mutateAsync({ id: e.id, fields });
        toast("已保存");
      } else {
        await m.create.mutateAsync(fields);
        toast(`已添加「${d.name.trim()}」`);
      }
      props.onDone();
    } catch (err) {
      setError(errorText(err));
    }
  };
  const del = async () => {
    if (!e || !(await confirm({ title: `删除「${e.name}」？`, confirmLabel: "删除", danger: true }))) return;
    m.remove.mutate(e.id, {
      onSuccess: () => {
        toast("已删除");
        props.onDone();
      },
    });
  };
  const generate = async () => {
    try {
      set("password", await vaultApi.generate());
      setShow(true);
    } catch (err) {
      toast(errorText(err), "error");
    }
  };

  return (
    <SidePanel
      title={e ? "编辑密码" : "新密码"}
      onClose={props.onClose}
      onSubmit={save}
      onDelete={e ? () => void del() : undefined}
      error={error}
      meta={e ? `添加于 ${day(e.createdAt)} · 修改于 ${day(e.updatedAt)}` : undefined}
      extra={confirmDialog}
    >
      <Field label="名称">
        <TextInput autoFocus value={d.name} onChange={(x) => set("name", x.target.value)} placeholder="必填" maxLength={100} />
      </Field>
      <Field label="链接">
        <div className="flex items-center gap-1">
          <TextInput aria-label="链接" value={d.link} onChange={(x) => set("link", x.target.value)} maxLength={1000} />
          <IconButton label="打开链接" disabled={!d.link.trim()} onClick={() => void ipc.openLink(d.link).catch((err) => toast(errorText(err), "error"))}>
            <ExternalLink size={14} />
          </IconButton>
        </div>
      </Field>
      <Field label="用户名">
        <TextInput value={d.username} onChange={(x) => set("username", x.target.value)} maxLength={200} />
      </Field>
      <Field label="账号">
        <TextInput value={d.account} onChange={(x) => set("account", x.target.value)} maxLength={200} />
      </Field>
      <Field label="密码">
        <div className="flex items-center gap-1">
          <TextInput
            aria-label="密码"
            type={show ? "text" : "password"}
            autoComplete="new-password"
            value={d.password}
            onChange={(x) => set("password", x.target.value)}
            maxLength={1000}
            className="font-mono"
          />
          <IconButton label={show ? "隐藏密码" : "显示密码"} onClick={() => setShow(!show)}>
            {show ? <EyeOff size={14} /> : <Eye size={14} />}
          </IconButton>
          <IconButton label="随机生成" onClick={() => void generate()}>
            <WandSparkles size={14} />
          </IconButton>
        </div>
      </Field>
      <Field label="备注">
        <TextArea rows={4} value={d.note} onChange={(x) => set("note", x.target.value)} maxLength={5000} />
      </Field>
    </SidePanel>
  );
}
