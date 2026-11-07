// 设置 › 外观：配色（5 套）和明暗。改了马上换，主窗口和速记小窗都跟着（shared/appearance.ts）。
import { PALETTES, useAppearance, type ThemeChoice } from "@/shared/appearance";
import { Segmented } from "@/shared/ui";
import { Row, Section } from "./layout";

const THEMES: { value: ThemeChoice; label: string }[] = [
  { value: "system", label: "跟随系统" },
  { value: "light", label: "浅色" },
  { value: "dark", label: "深色" },
];

export function AppearanceSection() {
  const [a, set] = useAppearance();
  // the swatches show the mode in effect (shared/appearance.ts keeps it on the root)
  const mode = document.documentElement.dataset.theme === "dark" ? "dark" : "light";
  return (
    <Section title="外观">
      <Row label="配色" help="5 套配色，每套都有浅色和深色。速记小窗跟着一起换。">
        <div role="radiogroup" aria-label="配色" className="flex flex-wrap gap-3 py-1">
          {PALETTES.map((p) => {
            const on = p.id === a.palette;
            return (
              <button
                key={p.id}
                type="button"
                role="radio"
                aria-checked={on}
                onClick={() => set({ palette: p.id })}
                className={`flex w-[104px] flex-col gap-1.5 rounded-control p-1.5 text-left outline-none transition-colors duration-150 focus-visible:ring-2 focus-visible:ring-ring ${on ? "bg-accent" : "hover:bg-muted"}`}
              >
                <span className="relative block h-14 overflow-hidden rounded-md border" style={{ background: p.desk[mode] }} aria-hidden>
                  <span className="absolute inset-x-2 top-2 bottom-0 rounded-t-[5px]" style={{ background: p.paper[mode] }} />
                  <span className="absolute left-4 right-6 top-6 h-1.5 rounded-full" style={{ background: p.marker[mode], opacity: 0.9 }} />
                  <span className="absolute left-4 right-8 top-[26px] h-[2px] rounded-full" style={{ background: p.pen[mode] }} />
                  <span className="absolute left-4 bottom-2 h-3 w-8 rounded-[3px]" style={{ background: p.primary[mode] }} />
                </span>
                <span className={`text-xs leading-snug ${on ? "text-foreground" : "text-muted-foreground"}`}>{p.label}</span>
              </button>
            );
          })}
        </div>
      </Row>
      <Row label="明暗">
        <Segmented ariaLabel="明暗" value={a.theme} options={THEMES} onChange={(theme) => set({ theme })} />
      </Row>
    </Section>
  );
}
