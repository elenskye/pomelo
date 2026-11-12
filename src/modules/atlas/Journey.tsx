// One 年度小报, opened: the design the run was made for (designs.ts) draws it inside a frame of its own. The frame's
// document is the design's (`design.frame()`: the Design System's stylesheet and painter as they were when the design
// was frozen, and the design's own styles), so the app's styles don't reach the printed pages, the design's don't reach
// the app, and what shows is what was approved on visual-atelier's canvas — also years later, with newer designs around.
// Covers the desk under the top bar (a layer of its own on the desk, like 贴纸簿). Same origin: the design is mounted
// from here and tells this page what was clicked (在模块里打开, 收进抽屉, Esc).
import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { errorText } from "@/shared/feedback";
import { reducedMotion } from "@/shared/hand";
import { useAtlasRun } from "./api";
import { loadDesign, type Design, type Hooks, type Mounted } from "./designs";

/** `design`: draw it with this design instead of the run's own (Atlas's 样式 view: a look at a design). */
export function Journey(props: { id: number; desk: boolean; design?: string; onOpenRecord: (key: string) => void; onClose: () => void }) {
  const { data: run, error } = useAtlasRun(props.id);
  const frame = useRef<HTMLIFrameElement>(null);
  const [design, setDesign] = useState<Design | null>(null);
  const [failed, setFailed] = useState<string | null>(null);
  const hooks = useRef<Hooks>({});
  hooks.current = { openRecord: props.onOpenRecord, onClose: props.onClose, onEscape: props.onClose };
  const layer = document.querySelector(".desk");

  useEffect(() => {
    if (!run) return;
    let gone = false;
    void loadDesign(props.design ?? run.design)
      .then((d) => !gone && setDesign(d))
      .catch((e) => !gone && setFailed(errorText(e)));
    return () => void (gone = true);
  }, [run, props.design]);

  useEffect(() => {
    const el = frame.current;
    if (!run || !design || !el) return;
    let mounted: Mounted | null = null;
    const look = () => ({ palette: document.documentElement.dataset.palette ?? "", theme: document.documentElement.dataset.theme ?? "light", reduce: reducedMotion() });
    const mount = () => {
      const body = el.contentDocument?.body;
      if (!body || mounted) return;
      try {
        mounted = design.mount(body, run, {
          ...look(),
          desk: props.desk,
          backdrop: true,
          hooks: { openRecord: (key) => hooks.current.openRecord?.(key), onClose: () => hooks.current.onClose?.(), onEscape: () => hooks.current.onEscape?.() },
        });
        el.contentWindow?.focus();
        // the app's own shortcuts (⌘K, ⌘J, ⌘Z…) are heard on this window: pass them on from inside the frame
        el.contentDocument?.addEventListener("keydown", (e) => {
          if (e.metaKey || e.ctrlKey) window.dispatchEvent(new KeyboardEvent("keydown", { key: e.key, code: e.code, metaKey: e.metaKey, ctrlKey: e.ctrlKey, shiftKey: e.shiftKey, altKey: e.altKey, bubbles: true, cancelable: true }));
        });
      } catch (e) {
        setFailed(errorText(e));
      }
    };
    el.addEventListener("load", mount);
    el.srcdoc = design.frame();
    // the app's palette / light-dark changed while it's open
    const watch = new MutationObserver(() => mounted?.set(look()));
    watch.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme", "data-palette"] });
    return () => {
      el.removeEventListener("load", mount);
      watch.disconnect();
      mounted?.destroy();
    };
  }, [run, design, props.desk]);

  // Esc leaves when the keyboard is out here (inside the frame the design hears it first: a 纸条 closes before the 小报)
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && !document.querySelector("dialog[open]") && props.onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [props.onClose]); // eslint-disable-line react-hooks/exhaustive-deps

  if (!layer) return null;
  const why = failed ?? (error ? errorText(error) : null);
  return createPortal(
    <div className="atlas-journey" role="dialog" aria-label="年度小报">
      {why ? (
        <div className="absolute inset-0 grid place-items-center">
          <div className="space-y-3 text-center">
            <p className="selectable text-sm text-destructive">{why}</p>
            <button type="button" className="pm-link text-sm" onClick={props.onClose}>
              回到 Atlas
            </button>
          </div>
        </div>
      ) : (
        // named by aria-label, not title: WebKit shows an iframe's title as a gray tooltip wherever the pointer rests in it
        <iframe ref={frame} aria-label="年度小报" className="absolute inset-0 size-full border-0" />
      )}
    </div>,
    layer,
  );
}
