// 助手 (v0.4.5 阶段 8; Rust picky/assistants/): small agents with their own page, listed in 小拾's left column
// between 项目 and 定时. Built in: Translator, Pub2MD and Seeker. Their names are the user's to choose.
import { FileUser, Languages, ScanText, type LucideIcon } from "lucide-react";
import { SeekerPage } from "./Seeker";
import { Pub2mdPage } from "./Pub2md";
import { TranslatePage } from "./Translate";

export const ASSISTANTS: { key: string; name: string; icon: LucideIcon }[] = [
  { key: "translate", name: "Translator", icon: Languages },
  { key: "pub2md", name: "Pub2MD", icon: ScanText },
  { key: "seeker", name: "Seeker", icon: FileUser },
];

/** The page of the open 助手. */
export function AssistantPage(props: { which: string }) {
  if (props.which === "translate") return <TranslatePage />;
  if (props.which === "pub2md") return <Pub2mdPage />;
  if (props.which === "seeker") return <SeekerPage />;
  return <div className="flex-1" />;
}
