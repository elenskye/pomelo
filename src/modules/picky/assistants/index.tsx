// 助手 (v0.4.5 阶段 8; Rust picky/assistants/): small agents with their own page. Built in: Translator,
// Pub2MD, Seeker and Rater; their names are the user's to choose. Since v0.7 each one is a module of its
// own (`/translate`, `/pub2md`, `/seeker`, `/rater`; app/modules.ts), still listed in 小拾's left column.
import { Bird } from "lucide-react";
import { moduleById } from "@/app/modules";
import { PageHeader } from "@/shared/ui";
import { Pub2mdPage } from "./Pub2md";
import { RaterPage } from "./Rater";
import { SeekerPage } from "./Seeker";
import { TranslatePage } from "./Translate";

export const ASSISTANTS = ["translate", "pub2md", "seeker", "rater"].map((key) => {
  const m = moduleById(key);
  return { key, name: m.label, icon: m.icon };
});

/** A 助手 as a module: its title (with 小拾's bird) over its page. */
export default function AssistantModule(props: { which: "translate" | "pub2md" | "seeker" | "rater" }) {
  const m = moduleById(props.which);
  return (
    <div className="relative flex h-full min-h-0 flex-col">
      <PageHeader icon={m.icon} color={m.color} title={m.label} badge={<Bird size={14} strokeWidth={1.75} aria-label="小拾的助手" style={{ color: "var(--module-picky)" }} />} />
      {props.which === "translate" ? <TranslatePage /> : props.which === "pub2md" ? <Pub2mdPage /> : props.which === "rater" ? <RaterPage /> : <SeekerPage />}
    </div>
  );
}
