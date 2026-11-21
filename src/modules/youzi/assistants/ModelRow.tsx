// One AI function's row on an assistant's settings page (Rater's, Seeker's): a model (or 跟默认, when `fallback` is given)
// and what that model can do about thinking (LevelPicker). A row that follows the default shows no 思考 of its own.
import { Row } from "@/settings/layout";
import { Dropdown } from "@/shared/Dropdown";
import { useModels } from "../Schedules";
import { LevelPicker, fromLevel, levelOf } from "../Thinking";

export type ModelChoice = { model: string | null; thinking: boolean; effort?: string | null };

const SAME = "";

export function ModelRow(props: { label: string; name?: string; choice: ModelChoice | null; fallback?: ModelChoice; onChange: (c: ModelChoice | null) => void }) {
  const shown = props.choice ?? props.fallback ?? { model: null, thinking: false, effort: null };
  const { models, keep } = useModels(shown.model, shown.thinking);
  const own = models.filter((m) => m.value != null).map((m) => ({ ...m, value: m.value as string }));
  const options = props.fallback ? [{ value: SAME, label: "跟默认" }, ...own] : own;
  const follows = !!props.fallback && !props.choice;
  const name = props.name ?? props.label;
  const level = (
    <LevelPicker
      label={`${name} · 思考`}
      model={shown.model}
      exact
      value={levelOf(shown.thinking, shown.effort)}
      onChange={(l) => {
        const v = fromLevel(l);
        props.onChange({ model: keep(shown.model, v.thinking), ...v });
      }}
    />
  );
  return (
    <Row label={props.label} actions={follows ? undefined : level}>
      <Dropdown<string> block ariaLabel={`${name} · 模型`} value={props.choice?.model ?? (props.fallback ? SAME : "")} placeholder="选择模型" options={options} onChange={(m) => props.onChange(m === SAME && props.fallback ? null : { model: m, thinking: shown.thinking, effort: shown.effort })} />
    </Row>
  );
}
