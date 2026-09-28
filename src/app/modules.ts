// Sidebar/module registry. Add new modules here (id = route path segment).
import { BookOpen, Clapperboard, Luggage, Palette, type LucideIcon } from "lucide-react";

export type ModuleDef = { id: string; label: string; icon: LucideIcon; color: string };

export const MODULES: ModuleDef[] = [
  { id: "books", label: "摸鱼读读", icon: BookOpen, color: "var(--module-books)" },
  { id: "films", label: "好奇看看", icon: Clapperboard, color: "var(--module-films)" },
  { id: "drawing", label: "随意画画", icon: Palette, color: "var(--module-drawing)" },
  { id: "packing", label: "出门走走", icon: Luggage, color: "var(--module-packing)" },
];

export const moduleById = (id: string) => MODULES.find((m) => m.id === id)!;

export const DEFAULT_ROUTE = "/books";
