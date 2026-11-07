// The website's Markdown body on the shared live editor (shared/editor/markdown.ts: headings, lists, quotes,
// code, tables, Markdoc tags…, marks kept and dimmed); a line that is only an image shows the picture under it
// (the work's own images or http ones). The box keeps its own ⌘Z / ⌘⇧Z.
// Images dropped or pasted on it go into the work's image folder (Rust) and in as their own lines.
import { forwardRef, useImperativeHandle, useRef } from "react";
import { useDropTarget } from "@/shared/attachments/target";
import { LiveText, type LiveTextHandle } from "@/shared/editor/LiveText";
import { markdownFlavor } from "@/shared/editor/markdown";
import { siteSrc } from "./api";

export type MdEditorHandle = LiveTextHandle;

function imageSrc(url: string): string | null {
  if (url.startsWith("/") && !url.startsWith("//")) return siteSrc(url);
  return /^https?:\/\//.test(url) ? url : null;
}

const FLAVOR = markdownFlavor(imageSrc);

export const MdEditor = forwardRef<MdEditorHandle, { value: string; onChange: (v: string) => void; onImages: (take: { paths?: string[]; files?: File[] }) => Promise<string[]>; label: string; onError: (e: unknown) => void; readOnly?: boolean }>(function MdEditor(props, handle) {
  const box = useRef<HTMLDivElement>(null);
  const live = useRef<LiveTextHandle>(null);
  useImperativeHandle(handle, () => ({
    focus: () => live.current?.focus(),
    insertLines: (lines) => live.current?.insertLines(lines),
    selection: () => live.current?.selection() ?? [0, 0],
    replaceRange: (start, end, text) => live.current?.replaceRange(start, end, text),
  }));
  const take = (p: Promise<string[]>) => void p.then((urls) => live.current?.insertLines(urls.map((u) => `![](${u})`)), props.onError);
  const dropping = useDropTarget(box, { paths: (paths) => take(props.onImages({ paths })), files: (files) => take(props.onImages({ files })) }, !props.readOnly);
  return (
    <LiveText
      ref={live}
      boxRef={box}
      value={props.value}
      onChange={props.onChange}
      flavor={FLAVOR}
      label={props.label}
      readOnly={props.readOnly}
      focusAt="start"
      className={`min-h-[12em] rounded-control text-[15px] ${dropping ? "ring-2 ring-primary" : ""} ${props.readOnly ? "text-muted-foreground" : ""}`}
    />
  );
});
