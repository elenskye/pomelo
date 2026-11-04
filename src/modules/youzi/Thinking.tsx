// 思考 (rules in Rust: cloud.rs `Think`, `Level`, library.rs, schedule.rs): what a model can do about thinking (a
// cloud model's 思考 setting and 思考强度, a local one's catalog entry) and the composer's switch, kept per conversation
// (a draft's own until it's sent). A model that can't switch shows its fixed state, locked. One with 思考强度 turns the
// button into a menu of them; LevelPicker is the same choice where it's a setting (定时, the assistants, 设置). Both read
// the model chosen right now, so they change with it; a level the model doesn't have counts as its default.
import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { ChevronDown, Lightbulb } from "lucide-react";
import { Dropdown } from "@/shared/Dropdown";
import { errorText, useToast } from "@/shared/feedback";
import { LIBRARY_KEY, youziApi, useLibrary, useYouziStatus, type Level, type Library, type YouziStatus, type Think } from "./api";

export const THINK_LABELS: Record<Think, string> = { never: "不能思考", switch: "能开关", always: "只能思考" };

/** A model code's ability (local:<key> | cloud:<id>); unknown ones can switch. */
export function thinkOf(status: YouziStatus | undefined, code: string | null): Think {
  if (!status || !code) return "switch";
  if (code.startsWith("cloud:")) return status.cloud.find((m) => `cloud:${m.id}` === code)?.thinking ?? "switch";
  const key = code === "local" ? status.local.active?.key : code.slice(6);
  const m = status.local.models.find((x) => x.key === key);
  return m ? (m.thinking ? "switch" : "never") : "switch";
}

/** A model code's 思考强度 and the one fixed as its default (local models have none). */
export function levelsOf(status: YouziStatus | undefined, code: string | null): { efforts: string[]; fixed: string | null } {
  const m = code?.startsWith("cloud:") ? status?.cloud.find((x) => `cloud:${x.id}` === code) : undefined;
  return { efforts: m?.efforts ?? [], fixed: m?.defaultEffort ?? null };
}

/** The choices a model has: 不思考 (if it can), 思考 at its default, each level. */
export function levelOptions(status: YouziStatus | undefined, code: string | null): { value: Level; label: string }[] {
  const ability = thinkOf(status, code);
  const { efforts, fixed } = levelsOf(status, code);
  const off = { value: "off", label: "不思考" };
  if (ability === "never") return [off];
  const on = { value: "on", label: efforts.length ? `思考 · 默认${fixed ? `（${fixed}）` : ""}` : "思考" };
  return [...(ability === "switch" ? [off] : []), on, ...efforts.map((e) => ({ value: e, label: `思考 · ${e}` }))];
}

/** A chosen level as this model takes it. */
export function levelFor(status: YouziStatus | undefined, code: string | null, level: Level): Level {
  const options = levelOptions(status, code);
  if (options.some((o) => o.value === level)) return level;
  return options.some((o) => o.value === "on") ? "on" : "off";
}

/** 思考 as kept where it's a switch and a level (a schedule, an assistant's choice), and back. */
export const levelOf = (thinking: boolean, effort?: string | null): Level => (!thinking ? "off" : (effort ?? "on"));
export const fromLevel = (level: Level) => ({ thinking: level !== "off", effort: level === "off" || level === "on" ? null : level });

/** The choice of thinking for `model` (null: 柚子's current one; `exact`: none chosen yet), where it's a setting. */
export function LevelPicker(props: { label: string; model: string | null; exact?: boolean; value: Level; onChange: (v: Level) => void }) {
  const status = useYouziStatus().data;
  const code = props.model ?? (props.exact ? null : (status?.selection ?? null));
  return <Dropdown<Level> ariaLabel={props.label} value={levelFor(status, code, props.value)} options={levelOptions(status, code)} onChange={props.onChange} />;
}

/** Whether a model that can do `t` fits a task set to think (`on`) or not. */
export const thinkFits = (t: Think, on: boolean) => t === "switch" || (t === "always") === on;

function rowOf(lib: Library | undefined, id: number | null) {
  if (!lib || id == null) return undefined;
  return [...lib.pinned, ...lib.projects.flatMap((p) => p.sessions), ...lib.scheduled, ...lib.recent, ...lib.archived].find((s) => s.id === id);
}

/** The switch for `current` (a draft's own under `draftKey`, on until turned off). */
export function useThinking(current: number | null, draftKey: string) {
  const qc = useQueryClient();
  const toast = useToast();
  const lib = useLibrary().data;
  const [drafts, setDrafts] = useState<Record<string, boolean>>({});
  const [draftEfforts, setDraftEfforts] = useState<Record<string, string | null>>({});
  const on = current == null ? (drafts[draftKey] ?? true) : (rowOf(lib, current)?.thinking ?? true);
  const effort = current == null ? (draftEfforts[draftKey] ?? null) : (rowOf(lib, current)?.effort ?? null);
  const setEffort = (next: string | null) => {
    if (current == null) return setDraftEfforts((d) => ({ ...d, [draftKey]: next }));
    void youziApi.conversationEffort(current, next).then(
      () => qc.invalidateQueries({ queryKey: LIBRARY_KEY }),
      (e) => toast(errorText(e), "error"),
    );
  };
  const toggle = () => {
    if (current == null) return setDrafts((d) => ({ ...d, [draftKey]: !on }));
    void youziApi.conversationThinking(current, !on).then(
      () => qc.invalidateQueries({ queryKey: LIBRARY_KEY }),
      (e) => toast(errorText(e), "error"),
    );
  };
  /** What a new conversation is created with (null for an existing one). */
  const forSend = current == null ? on : null;
  const effortForSend = current == null ? effort : null;
  return { on, toggle, forSend, effort, setEffort, effortForSend };
}

/** The 思考 button for `model` (none: the selected one); a menu when the model has 思考强度. `onLevel` takes one
 * choice as a whole (a setting saved at once); else the composer's `onToggle` and `onEffort`. */
export function ThinkingButton(props: { model?: string | null; on: boolean; effort: string | null; onToggle?: () => void; onEffort?: (e: string | null) => void; onLevel?: (l: Level) => void; className: string }) {
  const status = useYouziStatus().data;
  const code = props.model ?? status?.selection ?? null;
  const ability = thinkOf(status, code);
  const fixed = ability !== "switch";
  const pressed = ability === "always" || (ability === "switch" && props.on);
  const { efforts, fixed: def } = levelsOf(status, code);
  if (efforts.length) {
    const chosen = props.effort && efforts.includes(props.effort) ? props.effort : null;
    const pick = (v: Level) => {
      if (props.onLevel) return props.onLevel(v);
      if ((v === "off") === props.on && ability === "switch") props.onToggle?.();
      if (v !== "off") props.onEffort?.(v === "on" ? null : v);
    };
    return (
      <Dropdown<Level>
        ariaLabel="思考强度"
        value={ability === "switch" && !props.on ? "off" : (chosen ?? "on")}
        options={levelOptions(status, code)}
        onChange={pick}
        renderTrigger={() => (
          <>
            <Lightbulb size={14} strokeWidth={1.5} /> 思考{pressed && (chosen ?? def) ? ` · ${chosen ?? def}` : ""}
            <ChevronDown size={12} strokeWidth={1.5} className="text-muted-foreground" />
          </>
        )}
        triggerClassName={`${props.className} ${pressed ? "bg-accent text-accent-foreground" : ""}`}
      />
    );
  }
  return (
    <button
      type="button"
      aria-pressed={pressed}
      aria-disabled={fixed}
      title={ability === "always" ? "这个模型只能思考" : ability === "never" ? "这个模型不能思考" : undefined}
      onClick={() => !fixed && (props.onLevel ? props.onLevel(props.on ? "off" : "on") : props.onToggle?.())}
      className={`${props.className} ${pressed ? "bg-accent text-accent-foreground" : ""} ${fixed ? "cursor-default opacity-50" : ""}`}
    >
      <Lightbulb size={14} strokeWidth={1.5} /> 思考
    </button>
  );
}
