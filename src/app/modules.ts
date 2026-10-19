// Sidebar/module registry. Add new modules here (id = route path segment).
import { Bird, BookOpen, Briefcase, CalendarHeart, Clapperboard, Feather, Footprints, KeyRound, Luggage, Palette, Shapes, Wallet, type LucideIcon } from "lucide-react";

export type ModuleDef = { id: string; label: string; icon: LucideIcon; color: string };

export const MODULES: ModuleDef[] = [
  { id: "todo", label: "一步一步", icon: Footprints, color: "var(--module-todo)" },
  { id: "books", label: "摸鱼读读", icon: BookOpen, color: "var(--module-books)" },
  { id: "films", label: "好奇看看", icon: Clapperboard, color: "var(--module-films)" },
  { id: "drawing", label: "随意画画", icon: Palette, color: "var(--module-drawing)" },
  { id: "packing", label: "出门走走", icon: Luggage, color: "var(--module-packing)" },
  { id: "finance", label: "钱来钱往", icon: Wallet, color: "var(--module-finance)" },
  { id: "jobs", label: "投石问路", icon: Briefcase, color: "var(--module-jobs)" },
  { id: "notes", label: "念念不忘", icon: Feather, color: "var(--module-notes)" },
  { id: "days", label: "来日方长", icon: CalendarHeart, color: "var(--module-days)" },
  { id: "vault", label: "芝麻开门", icon: KeyRound, color: "var(--module-vault)" },
  { id: "picky", label: "小拾", icon: Bird, color: "var(--module-picky)" },
];

/** Modules made from 五花八门 / 循序渐进 ("list/3"…), kept current by useAllModules (modules/custom/api.ts). */
let made: ModuleDef[] = [];
export const setMadeModules = (defs: ModuleDef[]) => {
  made = defs;
};

const find = (id: string) => MODULES.find((m) => m.id === id) ?? made.find((m) => m.id === id);

/** A built-in or made module; one deleted meanwhile reads as its key. */
export const moduleById = (id: string): ModuleDef => find(id) ?? { id, label: id, icon: Shapes, color: "var(--muted-foreground)" };

/** Chinese label for a module key (import summaries etc.); unknown keys pass through. */
export const moduleLabel = (id: string) => find(id)?.label ?? id;
