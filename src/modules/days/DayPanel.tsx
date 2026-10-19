// Side panel for one day: 类型 · 名称 · 日期 (生日: 年 (可不知道) / 月 / 日; 证件: 到期日) · 重复 (纪念日) ·
// 提醒 (up to 5 lead days) · 置顶 · 礼物点子 and 送过的 (生日) · 备注. Rules are checked in Rust.
import { useEffect, useMemo, useState, type FormEvent } from "react";
import { Plus, X } from "lucide-react";
import { DateInput } from "@/shared/DateInput";
import { Dropdown } from "@/shared/Dropdown";
import { currentYear, YearInput } from "@/shared/MonthInput";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { SidePanel } from "@/shared/panel";
import { Field, IconButton, Segmented, Switch, TextArea, TextInput } from "@/shared/ui";
import { DEFAULT_REMIND, KIND_LABEL, MAX_REMIND, REPEAT_LABEL, useDayMutations, type Day, type DayPatch, type Gift, type Kind, type Repeat } from "./api";

const LEADS = [0, 1, 3, 7, 14, 30, 60, 90];
const KINDS: Kind[] = ["day", "birthday", "document"];
const REPEATS: Repeat[] = ["none", "year", "month", "week"];
const DATE_LABEL: Record<Kind, string> = { day: "日期", birthday: "生日", document: "到期日" };

/** date: YYYY-MM-DD for 纪念日 / 证件; a birthday keeps year ("" = 不知道) / month / mday apart until saved. */
type Draft = { kind: Kind; name: string; date: string; year: string; month: string; mday: string; repeat: Repeat; remind: number[]; pinned: boolean; note: string; ideas: string; gifts: Gift[] };

const pad = (n: number) => String(n).padStart(2, "0");
const monthDays = (year: string, month: string) => (month ? new Date(Number(year) || 2000, Number(month), 0).getDate() : 31);
const stamp = (s: string) => {
  const d = new Date(s);
  return `${d.getFullYear()} / ${pad(d.getMonth() + 1)} / ${pad(d.getDate())}`;
};

function initial(d: Day | null, kind: Kind): Draft {
  if (!d) return { kind, name: "", date: "", year: "", month: "", mday: "", repeat: kind === "day" ? "none" : kind === "birthday" ? "year" : "none", remind: DEFAULT_REMIND[kind], pinned: false, note: "", ideas: "", gifts: [] };
  const birthday = d.kind === "birthday";
  return {
    kind: d.kind,
    name: d.name,
    date: birthday ? "" : d.date,
    year: birthday && !d.date.startsWith("--") ? d.date.slice(0, 4) : "",
    month: birthday ? d.date.slice(-5, -3) : "",
    mday: birthday ? d.date.slice(-2) : "",
    repeat: d.repeat,
    remind: d.remind,
    pinned: d.pinned,
    note: d.note ?? "",
    ideas: d.ideas ?? "",
    gifts: d.gifts,
  };
}

type Props = { day: Day | null; newKind: Kind; onClose: () => void; onDone: () => void; onDirtyChange: (dirty: boolean) => void };

export function DayPanel(props: Props) {
  const e = props.day;
  const start = useMemo(() => initial(e, props.newKind), [e]); // eslint-disable-line react-hooks/exhaustive-deps
  const [d, setD] = useState(start);
  const [error, setError] = useState<string | null>(null);
  const m = useDayMutations();
  const toast = useToast();
  const [confirm, confirmDialog] = useConfirm();
  const dirty = JSON.stringify(d) !== JSON.stringify(start);
  useEffect(() => {
    props.onDirtyChange(dirty);
  }, [dirty]); // eslint-disable-line react-hooks/exhaustive-deps
  const set = <K extends keyof Draft>(k: K, v: Draft[K]) => setD((x) => ({ ...x, [k]: v }));

  // the date carries over between kinds where it can; a new one's reminders follow its kind until changed
  const setKind = (kind: Kind) =>
    setD((x) => {
      if (kind === x.kind) return x;
      const next = { ...x, kind, remind: !e && JSON.stringify(x.remind) === JSON.stringify(DEFAULT_REMIND[x.kind]) ? DEFAULT_REMIND[kind] : x.remind };
      if (kind === "birthday" && x.kind !== "birthday" && x.date) return { ...next, year: x.date.slice(0, 4), month: x.date.slice(5, 7), mday: x.date.slice(8) };
      if (kind !== "birthday" && x.kind === "birthday" && x.year && x.month && x.mday) return { ...next, date: `${x.year}-${x.month}-${x.mday}` };
      return next;
    });
  const setMonth = (key: "year" | "month", v: string) =>
    setD((x) => {
      const y = { ...x, [key]: v };
      const max = monthDays(y.year, y.month);
      return y.mday && Number(y.mday) > max ? { ...y, mday: pad(max) } : y;
    });
  const toggleLead = (n: number) => set("remind", d.remind.includes(n) ? d.remind.filter((x) => x !== n) : [...d.remind, n].sort((a, b) => b - a));

  const date = () => {
    if (d.kind !== "birthday") return d.date;
    if (!d.month || !d.mday) return "";
    return d.year ? `${d.year}-${d.month}-${d.mday}` : `--${d.month}-${d.mday}`;
  };
  const save = async (ev: FormEvent) => {
    ev.preventDefault();
    setError(null);
    const day = date();
    if (!day) return setError(d.kind === "birthday" ? "请选生日的月和日" : `请选${DATE_LABEL[d.kind]}`);
    const t = (s: string) => s.trim() || null;
    const birthday = d.kind === "birthday";
    const patch: DayPatch = {
      kind: d.kind,
      name: d.name,
      date: day,
      repeat: d.repeat,
      remind: d.remind,
      pinned: d.pinned,
      note: t(d.note),
      ideas: birthday ? t(d.ideas) : null,
      gifts: birthday ? d.gifts.filter((g) => g.gift.trim()) : [],
    };
    try {
      if (e) {
        if (dirty) await m.update.mutateAsync({ id: e.id, patch });
        toast("已保存");
      } else {
        await m.create.mutateAsync(patch);
        toast(`已添加「${d.name.trim()}」`);
      }
      props.onDone();
    } catch (err) {
      setError(errorText(err));
    }
  };
  const del = async () => {
    if (!e) return;
    const ok = await confirm({ title: `删除「${e.name}」？`, message: e.gifts.length > 0 ? `送过的 ${e.gifts.length} 条礼物也会一起删掉。` : undefined, confirmLabel: "删除", danger: true });
    if (!ok) return;
    m.remove.mutate(e.id, {
      onSuccess: () => {
        toast("已删除");
        props.onDone();
      },
    });
  };

  const thisYear = new Date().getFullYear();
  const leads = [...new Set([...LEADS, ...d.remind])].sort((a, b) => a - b);
  const full = d.remind.length >= MAX_REMIND;

  return (
    <SidePanel
      title={e ? "编辑日子" : "新日子"}
      onClose={props.onClose}
      onSubmit={save}
      onDelete={e ? () => void del() : undefined}
      error={error}
      meta={e ? `添加于 ${stamp(e.createdAt)} · 修改于 ${stamp(e.updatedAt)}` : undefined}
      extra={confirmDialog}
    >
      <Field label="类型">
        <Segmented ariaLabel="类型" value={d.kind} onChange={setKind} options={KINDS.map((k) => ({ value: k, label: KIND_LABEL[k] }))} />
      </Field>
      <Field label="名称">
        <TextInput autoFocus value={d.name} onChange={(x) => set("name", x.target.value)} placeholder="必填" maxLength={50} />
      </Field>
      <Field label={DATE_LABEL[d.kind]}>
        {d.kind === "birthday" ? (
          <div className="flex items-center gap-2">
            <Dropdown<string>
              ariaLabel="年"
              value={d.year}
              onChange={(v) => setMonth("year", v)}
              menuMaxHeight={260}
              options={[{ value: "", label: "不知道哪年" }, ...Array.from({ length: thisYear - 1900 + 1 }, (_, i) => String(thisYear - i)).map((y) => ({ value: y, label: `${y} 年` }))]}
            />
            <Dropdown<string> ariaLabel="月" value={d.month} placeholder="月" onChange={(v) => setMonth("month", v)} menuMaxHeight={260} options={Array.from({ length: 12 }, (_, i) => ({ value: pad(i + 1), label: `${i + 1} 月` }))} />
            <Dropdown<string>
              ariaLabel="日"
              value={d.mday}
              placeholder="日"
              onChange={(v) => set("mday", v)}
              menuMaxHeight={260}
              options={Array.from({ length: monthDays(d.year, d.month) }, (_, i) => ({ value: pad(i + 1), label: `${i + 1} 日` }))}
            />
          </div>
        ) : (
          <DateInput ariaLabel={DATE_LABEL[d.kind]} value={d.date} onChange={(v) => set("date", v)} />
        )}
      </Field>
      {d.kind === "day" && (
        <Field label="重复">
          <Segmented ariaLabel="重复" value={d.repeat} onChange={(v) => set("repeat", v)} options={REPEATS.map((r) => ({ value: r, label: REPEAT_LABEL[r] }))} />
        </Field>
      )}
      <Field label="提醒">
        <div className="flex flex-wrap gap-1.5">
          {leads.map((n) => {
            const on = d.remind.includes(n);
            return (
              <button
                key={n}
                type="button"
                aria-pressed={on}
                disabled={!on && full}
                onClick={() => toggleLead(n)}
                className={`rounded-full border px-2.5 py-0.5 text-xs transition-colors duration-150 outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-40 ${
                  on ? "border-transparent bg-accent text-accent-foreground" : "text-muted-foreground enabled:hover:bg-muted"
                }`}
              >
                {n === 0 ? "当天" : `${n} 天前`}
              </button>
            );
          })}
        </div>
      </Field>
      <div className="flex items-center justify-between">
        <span className="text-xs text-muted-foreground">置顶</span>
        <Switch label="置顶" checked={d.pinned} onChange={(v) => set("pinned", v)} />
      </div>
      {d.kind === "birthday" && (
        <>
          <Field label="礼物点子">
            <TextArea rows={3} value={d.ideas} onChange={(x) => set("ideas", x.target.value)} maxLength={2000} />
          </Field>
          <Field label="送过的">
            <div className="space-y-2">
              {d.gifts.map((g, i) => {
                const put = (p: Partial<Gift>) => set("gifts", d.gifts.map((x, j) => (j === i ? { ...x, ...p } : x)));
                return (
                  <div key={i} className="flex items-center gap-2">
                    <YearInput clearable={false} value={g.year} onChange={(year) => put({ year: year ?? currentYear() })} />
                    <TextInput aria-label="送了什么" value={g.gift} onChange={(x) => put({ gift: x.target.value })} maxLength={100} />
                    <IconButton label="删掉这条" danger onClick={() => set("gifts", d.gifts.filter((_, j) => j !== i))}>
                      <X size={14} />
                    </IconButton>
                  </div>
                );
              })}
              <button type="button" onClick={() => set("gifts", [...d.gifts, { year: currentYear(), gift: "" }])} className="flex items-center gap-1 text-sm text-primary hover:underline">
                <Plus size={14} /> 记一份
              </button>
            </div>
          </Field>
        </>
      )}
      <Field label="备注">
        <TextArea rows={3} value={d.note} onChange={(x) => set("note", x.target.value)} maxLength={2000} />
      </Field>
    </SidePanel>
  );
}
