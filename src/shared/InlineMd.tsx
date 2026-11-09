// Renders the bits of Markdown titles and notes use: **bold**, and `:pm-<key>:` icons from the icon library
// (shared/editor/inlineIcon.ts). Everything else stays plain text.
import { LibIcon, libraryIcon } from "./iconLibrary";

export function InlineMd(props: { text: string }) {
  const parts = props.text.split(/(\*\*[^*\n]+\*\*|:pm-[a-z0-9-]+:)/);
  return (
    <>
      {parts.map((p, i) =>
        p.startsWith("**") && p.endsWith("**") && p.length > 4 ? (
          <b key={i} className="font-bold">
            <InlineMd text={p.slice(2, -2)} />
          </b>
        ) : p.startsWith(":pm-") && p.endsWith(":") && libraryIcon(p.slice(4, -1)) ? (
          <LibIcon key={i} name={p.slice(4, -1)} size={16} className="inline-block align-[-0.18em]" />
        ) : (
          p
        ),
      )}
    </>
  );
}
