// Stylesheets of the main window don't reach into each other (pnpm check; docs/testing.md「样式不串」).
// Every stylesheet loads at start (main.tsx), so a module's unscoped rule applies on every page: 日历's `.cd` (a day)
// once drew 见字如面's inline code 26px wide. In a module's stylesheet (src/modules/<m>/, src/calendar/, src/home/ …):
//   - a rule led by a lone class (`.x`, `.x:hover`, `.x::after`, `.x .y`) must not use a class name that another
//     module's stylesheet also uses (scope it under the module's own box, or give it the module's prefix);
//   - no rule led by a bare element (`p`, `table td`, `[data-x]`): that is everyone's.
// In every stylesheet of the app's windows: a box that cuts its words off (text-overflow: ellipsis, -webkit-line-clamp)
// clips with `overflow: clip`, not `hidden` — in WebKit a hidden box with words cut off catches the mouse wheel, and the
// list under the pointer doesn't scroll (index.css does the same for Tailwind's truncate / line-clamp-*).
// Shared stylesheets (index.css, app/frame.css, shared/**) are global on purpose; the print page and the 速记小窗 are
// documents of their own (their stylesheets are checked against nothing).
import { readFileSync } from "node:fs";
import { execSync } from "node:child_process";

const files = execSync("find src -name '*.css' -not -path 'src/vendor/*'", { cwd: new URL("..", import.meta.url) }).toString().trim().split("\n").sort();
const own = (f) => !/^src\/(index\.css|app\/frame\.css|shared\/|print\/|capture\/)/.test(f);
const mainWindow = (f) => !/^src\/(print|capture)\//.test(f);
const moduleOf = (f) => (f.match(/^src\/modules\/[^/]+\//) ?? f.match(/^src\/[^/]+\//) ?? [f])[0];

const root = new URL("..", import.meta.url);
const selectorsOf = (f) => {
  const css = readFileSync(new URL(f, root), "utf8")
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .replace(/@(media|supports|layer|container|scope)[^{]*\{/g, "")
    .replace(/@keyframes[^{]*\{(?:[^{}]*\{[^{}]*\})*[^{}]*\}/g, "");
  return [...css.matchAll(/([^{}@;]+)\{/g)].flatMap((m) => m[1].split(",").map((s) => s.trim())).filter(Boolean);
};
const classesIn = (s) => [...s.matchAll(/\.([a-zA-Z_][\w-]*)/g)].map((m) => m[1]);

const users = new Map(); // class → stylesheets that name it
const sels = new Map();
for (const f of files.filter(mainWindow)) {
  const ss = selectorsOf(f);
  sels.set(f, ss);
  for (const s of ss) for (const c of classesIn(s)) (users.get(c) ?? users.set(c, new Set()).get(c)).add(f);
}

const errors = [];
for (const f of files.filter((f) => mainWindow(f) && own(f))) {
  for (const s of sels.get(f)) {
    const lead = s.split(/\s*[\s>+~]\s*/)[0];
    // named highlights (::highlight(x)) are the module's by their name
    if (/^(:root|html|body)\b/.test(lead) || /^(from|to|\d+%)$/.test(lead) || lead.includes("::highlight(")) continue;
    const cs = classesIn(lead);
    if (!cs.length) {
      // led by an element or attribute: fine when a class further on picks the module's own things
      if (!classesIn(s).length && !/^\[data-theme/.test(lead)) errors.push(`${f}: "${s}" has no class (applies to every page)`);
      continue;
    }
    if (cs.length !== 1) continue;
    const others = [...(users.get(cs[0]) ?? [])].filter((g) => moduleOf(g) !== moduleOf(f));
    if (others.length) errors.push(`${f}: ".${cs[0]}" (in "${s}") is a whole-app rule, and ${others.join(", ")} uses that class too`);
  }
}

for (const f of files.filter((f) => !f.startsWith("src/print/"))) {
  const css = readFileSync(new URL(f, root), "utf8").replace(/\/\*[\s\S]*?\*\//g, "");
  for (const m of css.matchAll(/([^{}]+)\{([^{}]*)\}/g))
    if (/text-overflow:\s*ellipsis|-webkit-line-clamp/.test(m[2]) && /(^|[;\s])overflow(-[xy])?:\s*hidden/.test(m[2])) errors.push(`${f}: "${m[1].trim()}" cuts words off with overflow: hidden (use clip: hidden catches the mouse wheel in WebKit)`);
}

if (errors.length) {
  console.error(`样式没过（${errors.length}）：\n${[...new Set(errors)].map((e) => `- ${e}`).join("\n")}`);
  process.exit(1);
}
console.log(`css-lint: ${files.length} 个样式表，没有串的`);
