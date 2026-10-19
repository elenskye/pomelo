// 助手 (v0.4.5 阶段 8; Rust picky/assistants/): small agents with their own page, listed in 小拾's left column
// between 项目 and 定时. Built in: 翻译 (Work comes next, Pub2MD later).
import { Languages, type LucideIcon } from "lucide-react";
import { TranslatePage } from "./Translate";

export const ASSISTANTS: { key: string; name: string; icon: LucideIcon }[] = [{ key: "translate", name: "翻译", icon: Languages }];

/** The page of the open 助手. */
export function AssistantPage(props: { which: string }) {
  return props.which === "translate" ? <TranslatePage /> : <div className="flex-1" />;
}
