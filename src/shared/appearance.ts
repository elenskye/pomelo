// 外观：配色（5 套，visual-atelier 的 schemes/pomelo）和明暗。设置存在 Rust 的 settings 表（键 `appearance`，不进撤销），
// 另在 localStorage 留一份，打开窗口时先用它，免得先闪一下默认的蓝笔。
// 根元素的 data-palette 选配色；data-theme 永远写成实际的明暗（选「跟随系统」时跟着系统变），所以 CSS 和 Tailwind 的
// dark: 只看 data-theme（index.css 的 @custom-variant）。颜色本身都在 src/vendor/pomelo/pomelo.css。
import { useEffect, useState } from "react";
import { tokens } from "@/vendor/pomelo/tokens";
import { api } from "./ipc";

export type Palette = keyof typeof tokens.palettes;
export type ThemeChoice = "system" | "light" | "dark";
export type Appearance = { palette: Palette; theme: ThemeChoice };

const KEY = "appearance";
const CACHE = "pomelo.appearance";
export const DEFAULT_APPEARANCE: Appearance = { palette: tokens.defaultPalette, theme: "system" };

/** The palettes in the scheme's order, with the colours a swatch needs (light and dark). */
export const PALETTES = (Object.keys(tokens.palettes) as Palette[]).map((id) => {
  const p = tokens.palettes[id];
  const c = (role: keyof typeof p.color) => p.color[role];
  return { id, label: p.label, desk: c("desk"), paper: c("paper"), pen: c("pen"), primary: c("primary"), marker: c("marker"), ink: c("ink") };
});

export const isAppearance = (v: unknown): v is Appearance => {
  if (!v || typeof v !== "object") return false;
  const a = v as Record<string, unknown>;
  return typeof a.palette === "string" && a.palette in tokens.palettes && (a.theme === "system" || a.theme === "light" || a.theme === "dark");
};

const dark = () => window.matchMedia("(prefers-color-scheme: dark)");
let current: Appearance = DEFAULT_APPEARANCE;
const listeners = new Set<(a: Appearance) => void>();

function paint() {
  const root = document.documentElement;
  root.dataset.palette = current.palette;
  const mode = current.theme === "system" ? (dark().matches ? "dark" : "light") : current.theme;
  root.dataset.theme = mode;
  root.style.colorScheme = mode;
}

function apply(a: Appearance, { save = false } = {}) {
  current = a;
  paint();
  try {
    localStorage.setItem(CACHE, JSON.stringify(a));
  } catch {
    /* private storage off: the stored setting still loads */
  }
  if (save) api.settingsSet(KEY, a).catch(() => {});
  for (const l of listeners) l(a);
}

/** Once per window, before the first render: the cached choice now, the stored one when it loads. */
export function startAppearance() {
  try {
    const cached = JSON.parse(localStorage.getItem(CACHE) ?? "null");
    if (isAppearance(cached)) current = cached;
  } catch {
    /* no cache */
  }
  paint();
  dark().addEventListener("change", () => current.theme === "system" && paint());
  const load = () =>
    api
      .settingsGet<unknown>(KEY)
      .then((v) => {
        const a = isAppearance(v) ? v : DEFAULT_APPEARANCE;
        if (a.palette !== current.palette || a.theme !== current.theme) apply(a);
      })
      .catch(() => {});
  void load();
  // another window (设置 in the main window, 速记小窗) may have changed it meanwhile
  window.addEventListener("focus", () => void load());
}

/** [appearance, change] for the 设置 page; a change merges onto the latest choice, paints at once and is stored. */
export function useAppearance() {
  const [value, setValue] = useState(current);
  useEffect(() => {
    listeners.add(setValue);
    setValue(current);
    return () => {
      listeners.delete(setValue);
    };
  }, []);
  return [value, (change: Partial<Appearance>) => apply({ ...current, ...change }, { save: true })] as const;
}
