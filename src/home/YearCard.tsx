// 首页 in the new year's first week (1 月 1～7 日; rule in Rust youzi/atlas.rs `envelope`): an envelope on the desk,
// sealed with a 柚子 sticker — last year's 年度小报. A click opens it in Atlas, envelope first (拆开 is the ritual).
// The envelope is drawn by the design the run was made for (modules/atlas/designs.ts). Hidden when 柚子 is hidden
// (the note it sits in is) or Atlas is.
import { useEffect, useState } from "react";
import { useHiddenModules } from "@/app/hidden";
import { useTabs } from "@/app/tabs";
import { useAtlasOverview } from "@/modules/atlas/api";
import { loadDesign } from "@/modules/atlas/designs";
import type { Hit } from "@/shared/open";
import "@/modules/atlas/atlas.css";

export function YearCard() {
  const [hidden] = useHiddenModules();
  const { data: o } = useAtlasOverview();
  const tabs = useTabs();
  const run = o?.envelope != null ? o.entries.find((e) => e.id === o.envelope) : undefined;
  const [html, setHtml] = useState<string | null>(null);
  useEffect(() => {
    if (!run) return setHtml(null);
    let gone = false;
    void loadDesign(run.design)
      .then((d) => !gone && setHtml(d.envelope(run.year)))
      .catch(() => {});
    return () => void (gone = true);
  }, [run?.id, run?.design, run?.year]);
  if (!run || !html || hidden.includes("atlas")) return null;
  const open = () => {
    const hit: Hit = { module: "atlas", kind: "envelope", id: run.id, title: "", detail: null, parent: null, day: null, amount: null, currency: null, flow: null };
    tabs.open("/atlas", { open: hit });
  };
  return (
    <div
      role="button"
      tabIndex={0}
      data-nodrag
      aria-label={`拆开：${run.year} 年度小报`}
      className="atlas-envelope pm-press outline-none focus-visible:ring-2 focus-visible:ring-ring"
      onClick={open}
      onKeyDown={(e) => (e.key === "Enter" || e.key === " ") && (e.preventDefault(), open())}
    >
      <span dangerouslySetInnerHTML={{ __html: html }} />
    </div>
  );
}
