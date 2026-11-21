// 新建 / 改名和图标 of a made module: 名称, 图标和颜色 (any sticker, one of the 5 category colors; app/LookPicker),
// and for 循序渐进 the two levels' names. A sticker drawn for one of the 24 icons (custom.rs ICONS) is kept as the
// module's icon; any other is kept with its color in settings (app/looks.ts).
import { useState, type FormEvent } from "react";
import { useNavigate } from "react-router";
import { freeTint, lookFor, lookKey, useLooks } from "@/app/looks";
import { LookPicker } from "@/app/LookPicker";
import { takenBy, useModuleLooks } from "@/app/moduleLooks";
import { errorText, useToast } from "@/shared/feedback";
import { stickerById, type Tint } from "@/shared/marks";
import { Modal } from "@/shared/Modal";
import { Field, TextInput } from "@/shared/ui";
import { TEMPLATE_LABEL, useCustomMutations, type Custom, type Template } from "./api";
import { iconForMark, MADE_MARKS } from "./icons";

const ownOf = (icon: string) => {
  const mark = MADE_MARKS[icon] ?? "shapes";
  return { mark, tint: stickerById(mark)?.tint ?? null };
};

export function CustomDialog(props: { template: Template; custom?: Custom; onClose: () => void; /** A new one is made: where it goes in the 模块坞. */ onMade?: (key: string) => void }) {
  const c = props.custom;
  const [name, setName] = useState(c?.name ?? "");
  const looks = useLooks();
  // no two modules alike (app/looks.ts): what the others wear is greyed out; a new one starts in a free color
  const { defs, eff } = useModuleLooks();
  const taken = takenBy(defs, eff, c?.key ?? "");
  const now = c ? eff.get(c.key) : undefined;
  const [mark, setMark] = useState(now?.mark ?? (c ? ownOf(c.icon).mark : "shapes"));
  const [tint, setTint] = useState<Tint | null>(() => (now ? now.tint : freeTint("shapes", ownOf("shapes").tint, taken)));
  const [words, setWords] = useState({ course: c?.courseWord || "课程", lesson: c?.lessonWord || "课节" });
  const [error, setError] = useState<string | null>(null);
  const m = useCustomMutations();
  const toast = useToast();
  const navigate = useNavigate();
  const progress = props.template === "progress";

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    const who = taken.get(lookKey(mark, tint));
    if (who) return setError(`图标和颜色和「${who}」一模一样了，换一张贴纸或一种颜色`);
    const icon = iconForMark(mark) ?? c?.icon ?? "shapes";
    const patch = { name, icon, ...(progress ? { courseWord: words.course, lessonWord: words.lesson } : {}) };
    try {
      if (c) {
        await m.update.mutateAsync({ id: c.id, patch });
        looks.set(c.key, lookFor(ownOf(icon), mark, tint));
        toast("已保存");
      } else {
        const made = await m.create.mutateAsync({ template: props.template, ...patch });
        looks.set(made.key, lookFor(ownOf(icon), mark, tint));
        toast(`已新建「${made.name}」`);
        props.onMade?.(made.key);
        void navigate(`/${made.key}`);
      }
      props.onClose();
    } catch (err) {
      setError(errorText(err));
    }
  };

  return (
    <Modal title={c ? "改名和图标" : `新建${TEMPLATE_LABEL[props.template]}`} wide onClose={props.onClose} onSubmit={submit} error={error} submitLabel={c ? "保存" : "新建"}>
      <Field label="名称">
        <TextInput autoFocus value={name} onChange={(e) => setName(e.target.value)} placeholder="必填" maxLength={20} />
      </Field>
      <Field label="图标和颜色">
        <LookPicker mark={mark} tint={tint} taken={taken} onMark={setMark} onTint={setTint} />
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
