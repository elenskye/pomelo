// 思考 (rules in Rust: cloud.rs `Think`, library.rs, schedule.rs): what a model can do about thinking (a
// cloud model's 思考 setting, a local one's catalog entry) and the composer's switch, kept per conversation
// (a draft's own until it's sent). A model that can't switch shows its fixed state, locked.
import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { Lightbulb } from "lucide-react";
import { errorText, useToast } from "@/shared/feedback";
import { LIBRARY_KEY, pickyApi, useLibrary, usePickyStatus, type Library, type PickyStatus, type Think } from "./api";

export const THINK_LABELS: Record<Think, string> = { never: "不能思考", switch: "能开关", always: "只能思考" };

/** A model code's ability (local:<key> | cloud:<id>); unknown ones can switch. */
export function thinkOf(status: PickyStatus | undefined, code: string | null): Think {
  if (!status || !code) return "switch";
  if (code.startsWith("cloud:")) return status.cloud.find((m) => `cloud:${m.id}` === code)?.thinking ?? "switch";
  const key = code === "local" ? status.local.active?.key : code.slice(6);
  const m = status.local.models.find((x) => x.key === key);
  return m ? (m.thinking ? "switch" : "never") : "switch";
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
  const on = current == null ? (drafts[draftKey] ?? true) : (rowOf(lib, current)?.thinking ?? true);
  const toggle = () => {
    if (current == null) return setDrafts((d) => ({ ...d, [draftKey]: !on }));
    void pickyApi.conversationThinking(current, !on).then(
      () => qc.invalidateQueries({ queryKey: LIBRARY_KEY }),
      (e) => toast(errorText(e), "error"),
    );
  };
  /** What a new conversation is created with (null for an existing one). */
  const forSend = current == null ? on : null;
  return { on, toggle, forSend };
}

/** The composer's 思考 button for the selected model. */
export function ThinkingButton(props: { on: boolean; onToggle: () => void; className: string }) {
  const status = usePickyStatus().data;
  const ability = thinkOf(status, status?.selection ?? null);
  const fixed = ability !== "switch";
  const pressed = ability === "always" || (ability === "switch" && props.on);
  return (
    <button
      type="button"
      aria-pressed={pressed}
      aria-disabled={fixed}
      title={ability === "always" ? "这个模型只能思考" : ability === "never" ? "这个模型不能思考" : undefined}
      onClick={() => !fixed && props.onToggle()}
      className={`${props.className} ${pressed ? "bg-accent text-accent-foreground" : ""} ${fixed ? "cursor-default opacity-50" : ""}`}
    >
      <Lightbulb size={14} strokeWidth={1.5} /> 思考
    </button>
  );
}
