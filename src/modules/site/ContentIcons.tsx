// 内容图标 (the website's own small pictures for its text, in place of emoji: visual-atelier's Vermilion scheme,
// DESIGN §6): written `:name:` in a Markdown body. The list (names, labels, the drawings) is synced with Pomelo's
// scheme as src/vendor/pomelo/content-icons.json — the one thing of Vermilion's Pomelo uses, and only here: the
// picker puts the name in at the caret; the website draws it. Pomelo's own interface doesn't use these.
import { useState } from "react";
import list from "@/vendor/pomelo/content-icons.json";
import { Smile } from "@/shared/icons";
import "./content-icons.css";

type ContentIcon = { name: string; label: { zh: string; en: string }; svg: string };
const ICONS = (list as { icons: ContentIcon[] }).icons;

/** The button that opens the picker, and the picker (a row of its own under the buttons). */
export function useContentIcons(insert: (text: string) => void) {
  const [open, setOpen] = useState(false);
  const button = (
    <button type="button" aria-expanded={open} onClick={() => setOpen(!open)} className={`ci-btn ${open ? "on" : ""}`}>
      <Smile size={14} /> 图标
    </button>
  );
  const panel = open && (
    <div className="ci-grid" role="listbox" aria-label="内容图标">
      {ICONS.map((i) => (
        <button
          key={i.name}
          type="button"
          role="option"
          aria-selected={false}
          title={`:${i.name}:`}
          onClick={() => {
            insert(`:${i.name}:`);
            setOpen(false);
          }}
          className="ci-cell"
        >
          <span aria-hidden className="ci-pic" dangerouslySetInnerHTML={{ __html: i.svg }} />
          <span className="ci-name">{i.label.zh}</span>
        </button>
      ))}
    </div>
  );
  return { button, panel };
}
