// 新建 / 改名和图标 of a made module: 名称, 图标 (24), and for 循序渐进 the two levels' names.
import { useState, type FormEvent } from "react";
import { useNavigate } from "react-router";
import { errorText, useToast } from "@/shared/feedback";
import { Modal } from "@/shared/Modal";
import { Field, TextInput } from "@/shared/ui";
import { TEMPLATE_COLOR, TEMPLATE_LABEL, useCustomMutations, type Custom, type Template } from "./api";
import { ICONS } from "./icons";

export function CustomDialog(props: { template: Template; custom?: Custom; onClose: () => void }) {
  const c = props.custom;
  const [name, setName] = useState(c?.name ?? "");
  const [icon, setIcon] = useState(c?.icon ?? ICONS[0][0]);
  const [words, setWords] = useState({ course: c?.courseWord || "课程", lesson: c?.lessonWord || "课节" });
  const [error, setError] = useState<string | null>(null);
  const m = useCustomMutations();
  const toast = useToast();
  const navigate = useNavigate();
  const progress = props.template === "progress";

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    const patch = { name, icon, ...(progress ? { courseWord: words.course, lessonWord: words.lesson } : {}) };
    try {
      if (c) {
        await m.update.mutateAsync({ id: c.id, patch });
        toast("已保存");
      } else {
        const made = await m.create.mutateAsync({ template: props.template, ...patch });
        toast(`已新建「${made.name}」`);
        void navigate(`/${made.key}`);
      }
      props.onClose();
    } catch (err) {
      setError(errorText(err));
    }
  };

  return (
    <Modal title={c ? "改名和图标" : `新建${TEMPLATE_LABEL[props.template]}`} onClose={props.onClose} onSubmit={submit} error={error} submitLabel={c ? "保存" : "新建"}>
      <Field label="名称">
        <TextInput autoFocus value={name} onChange={(e) => setName(e.target.value)} placeholder="必填" maxLength={20} />
      </Field>
      <Field label="图标">
        <div role="radiogroup" aria-label="图标" className="grid grid-cols-8 gap-1">
          {ICONS.map(([key, Icon]) => {
            const on = key === icon;
            return (
              <button
                key={key}
                type="button"
                role="radio"
                aria-checked={on}
                aria-label={key}
                onClick={() => setIcon(key)}
                className={`flex aspect-square items-center justify-center rounded-control transition-colors duration-150 outline-none focus-visible:ring-2 focus-visible:ring-ring ${on ? "bg-accent ring-1 ring-primary/40" : "hover:bg-muted"}`}
              >
                <Icon size={18} strokeWidth={1.5} style={{ color: TEMPLATE_COLOR[props.template] }} />
              </button>
            );
          })}
        </div>
      </Field>
      {progress && (
        <Field label="两层的叫法">
          <div className="flex items-center gap-2">
            <TextInput aria-label="上一层" value={words.course} onChange={(e) => setWords({ ...words, course: e.target.value })} maxLength={6} className="w-24" />
            <span className="text-muted-foreground">→</span>
            <TextInput aria-label="下一层" value={words.lesson} onChange={(e) => setWords({ ...words, lesson: e.target.value })} maxLength={6} className="w-24" />
          </div>
        </Field>
      )}
    </Modal>
  );
}
