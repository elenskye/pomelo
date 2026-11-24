// Tag icons: one from the icon library (shared/iconLibrary.tsx, kept as "i:<key>"); no icon = #. An emoji typed
// before the library replaced the emoji box is still shown.
//
// In a text (TagIcons below): 念念不忘's and 见字如面's #tags drawn the way the tag tree shows them (as Bear does): a tag with an icon
// has it in place of its 「#」. The character stays (the caret, copying and saving still see `#`); only its look is the
// icon (`.pm-hash`, text made transparent, the icon a mask in the text's colour). A tag without an icon of its own takes
// its nearest parent's. One <style> per window, from the two modules' tag lists; the drawers (lite.ts, NoteBody,
// noteHtml.ts, bear.ts) mark each tag with `data-tags` (notes / articles) and `data-tag`.
import { useArticlesOverview } from "@/modules/articles/api";
import { libraryIcon, StoredIcon } from "@/shared/iconLibrary";
import { useOverview, type TagCount } from "./api";

/** A tag's icon in a 16px slot: library icon (module color), an old emoji, or a muted #. */
export function TagIcon(props: { icon: string | null }) {
  return <StoredIcon icon={props.icon} size={14} color="var(--module-notes)" />;
}

const mask = (body: string) => `url("data:image/svg+xml;utf8,${encodeURIComponent(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none">${body}</svg>`)}")`;
const parent = (tag: string) => (tag.includes("/") ? tag.slice(0, tag.lastIndexOf("/")) : "");

/** Each tag's icon: its own, else its nearest parent's. */
function iconsOf(tags: TagCount[]): [string, string][] {
  const own = new Map(tags.filter((t) => t.icon).map((t) => [t.tag, t.icon!]));
  return tags.flatMap((t): [string, string][] => {
    for (let p = t.tag; p; p = parent(p)) {
      const icon = own.get(p);
      if (icon) return [[t.tag, icon]];
    }
    return [];
  });
}

function css(lists: [string, TagCount[]][]): string {
  const drawn: string[] = [];
  const typed: string[] = [];
  const each: string[] = [];
  for (const [scope, tags] of lists)
    for (const [tag, icon] of iconsOf(tags)) {
      const sel = `.pm-hashtag[data-tags="${scope}"][data-tag="${CSS.escape(tag)}"] > .pm-hash`;
      const key = icon.startsWith("i:") ? icon.slice(2) : null;
      const e = key ? libraryIcon(key) : null;
      if (e) {
        drawn.push(sel);
        each.push(`${sel}{--tag-ico:${mask(e.body)}}`);
      } else if (!key) {
        typed.push(sel);
        each.push(`${sel}::before{content:"${icon.replace(/["\\]/g, "")}"}`);
      }
    }
  const all = [...drawn, ...typed];
  if (!all.length) return "";
  const before = (sels: string[]) => sels.map((s) => `${s}::before`).join(",");
  return [
    `${all.join(",")}{display:inline-block;position:relative;width:1.15em;-webkit-text-fill-color:transparent}`,
    `${before(all)}{position:absolute;left:0.05em;top:50%;width:1em;height:1em;margin-top:-0.47em;line-height:1em;text-align:center}`,
    drawn.length ? `${before(drawn)}{content:"";background-color:currentColor;-webkit-mask:var(--tag-ico) center/contain no-repeat;mask:var(--tag-ico) center/contain no-repeat}` : "",
    typed.length ? `${before(typed)}{-webkit-text-fill-color:initial;font-size:0.9em}` : "",
    ...each,
  ].join("\n");
}

/** The window's tag icons (`articles`: also 见字如面's; the 速记小窗 has only 念念不忘's). */
export function TagIcons(props: { articles?: boolean }) {
  const notes = useOverview().data?.tags ?? [];
  const articles = useArticlesOverview(props.articles !== false).data?.tags ?? [];
  const text = css([["notes", notes], ["articles", props.articles === false ? [] : articles]]);
  return text ? <style>{text}</style> : null;
}
