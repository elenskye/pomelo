// 系统通知 (rules in Rust notify.rs): one row per kind — its switch and, for the three daily ones, its own time of day
// (定时任务 follows each task's own time). Only the installed app posts; other copies keep the settings.
import { useEffect, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { errorText, useToast } from "@/shared/feedback";
import { api, type NotifyKinds, type NotifySettings } from "@/shared/ipc";
import { Button, Switch, TextInput } from "@/shared/ui";
import { Row, Section } from "./layout";

const KEY = ["app", "notify"];
const LATE = "那时 Mac 在睡或柚儿园没开着，就等下次打开时补上。";

const KINDS: { key: keyof NotifyKinds; label: string; help: string; daily: boolean }[] = [
  { key: "todo", label: "一步一步", help: `今天到期和已过期的待办，每天到这个时间弹一条；${LATE}`, daily: true },
  { key: "jobs", label: "投石问路", help: `等回音满 14 天、该跟进的投递，每天到这个时间弹一条；${LATE}`, daily: true },
  { key: "days", label: "来日方长", help: `到了提前提醒的那几天和当天，还有过期的证件，每天到这个时间弹一条；${LATE}`, daily: true },
  { key: "schedule", label: "定时任务", help: "跟着每个定时任务自己的时间：写好了或没写成时弹，点了打开那个对话（年度总结打开 Atlas）。", daily: false },
];

/** HH:MM, saved on blur / Enter; Esc puts the saved one back. */
function TimeField(props: { label: string; value: string; disabled: boolean; onSave: (v: string) => Promise<unknown> }) {
  const [at, setAt] = useState(props.value);
  useEffect(() => setAt(props.value), [props.value]);
  const commit = () => {
    const v = at.trim();
    if (v !== props.value) void props.onSave(v).catch(() => setAt(props.value));
  };
  return (
    <TextInput
      aria-label={`${props.label} 每天几点`}
      value={at}
      maxLength={5}
      inputMode="numeric"
      disabled={props.disabled}
      onChange={(e) => setAt(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === "Enter") e.currentTarget.blur();
        if (e.key === "Escape") {
          setAt(props.value);
          e.currentTarget.blur();
        }
      }}
      className={`w-20 text-center tabular-nums ${props.disabled ? "opacity-40" : ""}`}
    />
  );
}

export function NotifySection() {
  const qc = useQueryClient();
  const toast = useToast();
  const { data } = useQuery({ queryKey: KEY, queryFn: api.notifySettings });
  if (!data) return null;
  const save = (change: { kinds?: NotifyKinds; kind?: string; at?: string }, next: NotifySettings) =>
    api.notifySet(change).then(
      () => qc.setQueryData(KEY, next),
      (e) => {
        toast(errorText(e), "error");
        throw e;
      },
    );
  const flip = (k: keyof NotifyKinds, on: boolean) => {
    const kinds = { ...data.kinds, [k]: on };
    void save({ kinds }, { ...data, kinds }).catch(() => {});
  };
  return (
    <Section title="通知" help={data.posts ? undefined : "这里只记下设置，正式版才会弹系统通知。"}>
      {data.allowed === false && (
        <Row label="系统设置" actions={<Button onClick={() => void api.notifyOpenSystem().catch((e) => toast(errorText(e), "error"))}>打开系统设置</Button>}>
          <span className="text-muted-foreground">系统设置里没允许柚儿园发通知</span>
        </Row>
      )}
      {KINDS.map((k) => (
        <Row key={k.key} label={k.label} help={k.help}>
          <div className="flex items-center gap-4">
            <Switch label={k.label} checked={data.kinds[k.key]} onChange={(on) => flip(k.key, on)} />
            {k.daily && (
              <TimeField
                label={k.label}
                value={data.times[k.key] ?? "09:00"}
                disabled={!data.kinds[k.key]}
                onSave={(at) => save({ kind: k.key, at }, { ...data, times: { ...data.times, [k.key]: at } })}
              />
            )}
          </div>
        </Row>
      ))}
    </Section>
  );
}
