// Sidebar/module registry. Add new modules here (id = route path segment).
import { Bird, BookOpen, Briefcase, Clapperboard, Feather, Footprints, KeyRound, Luggage, Palette, Wallet, type LucideIcon } from "lucide-react";

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
  { id: "vault", label: "芝麻开门", icon: KeyRound, color: "var(--module-vault)" },
  { id: "picky", label: "小拾", icon: Bird, color: "var(--module-picky)" },
];

export const moduleById = (id: string) => MODULES.find((m) => m.id === id)!;

/** Chinese label for a module key (import summaries etc.); unknown keys pass through. */
export const moduleLabel = (id: string) => MODULES.find((m) => m.id === id)?.label ?? id;
