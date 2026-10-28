// Module registry. Add new modules here (id = route path segment).
// MODULES hold data (search, export, 小拾 read them); the 助手 are 小拾's, shown as modules in the 模块坞, the
// 索引签 and the 柚子瓣 (their data and code stay in 小拾; hiding 小拾 hides them too).
import { Bird, BookOpen, Briefcase, CalendarHeart, Clapperboard, Compass, Feather, FileUser, MessageCircleQuestion, Footprints, House, KeyRound, Languages, LayoutGrid, Luggage, Palette, ScanText, Settings, Shapes, Wallet, type LucideIcon } from "lucide-react";

export type ModuleDef = {
  id: string;
  label: string;
  icon: LucideIcon;
  color: string;
  /** One of 小拾's 助手 (a small bird next to its name). */
  assistant?: boolean;
  /** Its pages (the path's second part, `/site/works`); each one is a step of 前进后退. */
  subs?: readonly { value: string; label: string }[];
};

export const MODULES: ModuleDef[] = [
  { id: "todo", label: "一步一步", icon: Footprints, color: "var(--module-todo)" },
  { id: "books", label: "摸鱼读读", icon: BookOpen, color: "var(--module-books)" },
  { id: "films", label: "好奇看看", icon: Clapperboard, color: "var(--module-films)" },
  { id: "drawing", label: "随意画画", icon: Palette, color: "var(--module-drawing)" },
  { id: "packing", label: "出门走走", icon: Luggage, color: "var(--module-packing)" },
  {
    id: "finance",
    label: "钱来钱往",
    icon: Wallet,
    color: "var(--module-finance)",
    subs: [
      { value: "entries", label: "收支" },
      { value: "assets", label: "资产" },
      { value: "reports", label: "报表" },
    ],
  },
  {
    id: "jobs",
    label: "投石问路",
    icon: Briefcase,
    color: "var(--module-jobs)",
    subs: [
      { value: "table", label: "投递" },
      { value: "stats", label: "统计" },
    ],
  },
  { id: "notes", label: "念念不忘", icon: Feather, color: "var(--module-notes)" },
  { id: "days", label: "来日方长", icon: CalendarHeart, color: "var(--module-days)" },
  {
    id: "site",
    label: "呦呦漫游",
    icon: Compass,
    color: "var(--module-site)",
    subs: [
      { value: "publish", label: "发布" },
      { value: "works", label: "作品" },
      { value: "now", label: "Now" },
      { value: "about", label: "关于我" },
      { value: "career", label: "职业" },
      { value: "assets", label: "资源库" },
      { value: "site", label: "站点" },
    ],
  },
  { id: "vault", label: "芝麻开门", icon: KeyRound, color: "var(--module-vault)" },
  { id: "picky", label: "小拾", icon: Bird, color: "var(--module-picky)" },
];

/** 小拾's 助手 as modules (names are the user's to choose). */
export const ASSISTANT_MODULES: ModuleDef[] = [
  {
    id: "translate",
    label: "Translator",
    icon: Languages,
    color: "var(--module-translate)",
    assistant: true,
    subs: [
      { value: "translate", label: "Translator" },
      { value: "prompt", label: "提示词" },
    ],
  },
  {
    id: "pub2md",
    label: "Pub2MD",
    icon: ScanText,
    color: "var(--module-pub2md)",
    assistant: true,
    subs: [
      { value: "work", label: "Pub2MD" },
      { value: "prompt", label: "提示词" },
    ],
  },
  {
    id: "seeker",
    label: "Seeker",
    icon: FileUser,
    color: "var(--module-seeker)",
    assistant: true,
    subs: [
      { value: "resume", label: "简历" },
      { value: "library", label: "素材" },
      { value: "dossiers", label: "档案" },
      { value: "rules", label: "规则" },
      { value: "apply", label: "网申" },
      { value: "history", label: "历史" },
      { value: "settings", label: "设置" },
    ],
  },
  {
    id: "rater",
    label: "Rater",
    icon: MessageCircleQuestion,
    color: "var(--module-rater)",
    assistant: true,
    subs: [
      { value: "bank", label: "题库" },
      { value: "cards", label: "卡片" },
      { value: "interview", label: "面试" },
      { value: "rules", label: "规则" },
    ],
  },
];

/** Pages that aren't modules: 首页, 模块坞, 设置. */
export const PLACES: ModuleDef[] = [
  { id: "", label: "首页", icon: House, color: "var(--muted-foreground)" },
  { id: "dock", label: "模块坞", icon: LayoutGrid, color: "var(--muted-foreground)" },
  { id: "settings", label: "设置", icon: Settings, color: "var(--muted-foreground)" },
];

/** Modules made from 五花八门 / 循序渐进 ("list/3"…), kept current by useAllModules (modules/custom/api.ts). */
let made: ModuleDef[] = [];
export const setMadeModules = (defs: ModuleDef[]) => {
  made = defs;
};

const find = (id: string) => MODULES.find((m) => m.id === id) ?? ASSISTANT_MODULES.find((m) => m.id === id) ?? made.find((m) => m.id === id) ?? PLACES.find((m) => m.id === id);

/** A built-in, 助手 or made module; one deleted meanwhile reads as its key. */
export const moduleById = (id: string): ModuleDef => find(id) ?? { id, label: id, icon: Shapes, color: "var(--muted-foreground)" };

/** Chinese label for a module key (import summaries etc.); unknown keys pass through. */
export const moduleLabel = (id: string) => find(id)?.label ?? id;

/** A path's module ("" = 首页; made ones are "list/3") and page. */
export function splitPath(path: string): { module: string; sub: string | null } {
  const parts = path.split("?")[0].split("/").filter(Boolean);
  if (parts[0] === "list" || parts[0] === "progress") return { module: parts.slice(0, 2).join("/"), sub: null };
  return { module: parts[0] ?? "", sub: parts[1] ?? null };
}

/** How a page reads in a 页签 and the history menu: 「Seeker · 简历」. */
export function pageName(path: string): string {
  const { module, sub } = splitPath(path);
  const m = moduleById(module);
  const s = sub ? m.subs?.find((x) => x.value === sub)?.label : null;
  // a 助手's own page is named like it: 「Translator」, not 「Translator · Translator」
  return s && s !== m.label ? `${m.label} · ${s}` : m.label;
}
