// Builds Pomelo's built-in résumé templates (src-tauri/templates/resume-cn.docx, resume-en.docx) from the
// user's two Seeker templates, read only: formatting only, placeholder text, no personal data, no thumbnail
// or metadata. Chinese: seek-it-cn-template tidied (A4, 等线, entry heading 名称 ⇥ 角色 ⇥ 时间 in one size, a navy rule under section
// titles); English: keyreply-resume-v5's styles with one sample paragraph per part. The sample paragraphs'
// order is output.rs `SLOTS_CN` / `SLOTS_EN`. Check the result for personal text before committing.
//   node scripts/resume-templates.ts [<Seeker folder>]
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { execFileSync } from "node:child_process";
const seeker = process.argv[2] ?? path.join(os.homedir(), "Documents/Seeker");
const work = fs.mkdtempSync(path.join(os.tmpdir(), "resume-templates-"));
const outDir = path.resolve(import.meta.dirname, "../src-tauri/templates");
for (const [lang, file] of [["cn", "cv/cn/seek-it-cn-template-v1.docx"], ["en", "cv/en/seek-it-keyreply-resume-v5.docx"]]) {
  fs.mkdirSync(path.join(work, lang));
  execFileSync("unzip", ["-q", path.join(seeker, file), "-d", path.join(work, lang)]);
}
process.chdir(work);
const W = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const read = (p: string) => fs.readFileSync(p, "utf8");
const paragraphsOf = (xml: string): string[] => xml.slice(xml.indexOf("<w:body>")).match(/<w:p[ >][\s\S]*?<\/w:p>/g);
const sectPrOf = (xml: string) => xml.match(/<w:sectPr[\s\S]*?<\/w:sectPr>/)[0];
const head = (xml: string) => xml.slice(0, xml.indexOf("<w:body>"));
const strip = (p: string) => p.replace(/<w:p\b[^>]*\/>/g, "").replace(/ w:rsid\w*="[^"]*"/g, "").replace(/<w:bookmark(Start|End)[^>]*\/>/g, "").replace(/<w:proofErr[^>]*\/>/g, "").replace(/<w:hyperlink[^>]*>|<\/w:hyperlink>/g, "");
const bold = (rpr: string) => /<w:b\/>|<w:b w:val="(1|true|on)"\/>/.test(rpr ?? "");
/** Placeholder text per tab column; per column the first text run gets it, the first run of the other weight a short word, the rest nothing. */
function sanitize(p: string, cols: string[]) {
  p = strip(p);
  let col = 0;
  const seen = []; // per column: weights kept
  return p.replace(/<w:r>([\s\S]*?)<\/w:r>|<w:r ([^>]*)>([\s\S]*?)<\/w:r>/g, (run, a, attrs, b) => {
    const inner = a ?? b;
    const rpr = inner.match(/<w:rPr>[\s\S]*?<\/w:rPr>/)?.[0] ?? "";
    const out = inner.replace(/<w:tab\/>|<w:t(?: [^>]*)?>[^<]*<\/w:t>/g, (tok) => {
      if (tok === "<w:tab/>") { col++; return tok; }
      const kept = (seen[col] ??= new Set());
      const w = bold(rpr) ? "b" : "p";
      let text = "";
      if (!kept.size) text = cols[col] ?? "text";
      else if (!kept.has(w)) text = w === "b" ? "Bold" : "text";
      if (text) kept.add(w);
      return `<w:t xml:space="preserve">${text}</w:t>`;
    });
    return `<w:r>${out}</w:r>`;
  });
}
function pack(dir: string, out: string, parts: Record<string, string>) {
  fs.rmSync(dir, { recursive: true, force: true });
  for (const [name, text] of Object.entries(parts)) {
    fs.mkdirSync(path.dirname(path.join(dir, name)), { recursive: true });
    fs.writeFileSync(path.join(dir, name), text);
  }
  fs.rmSync(out, { force: true });
  execFileSync("zip", ["-X", "-q", "-r", path.resolve(out), "[Content_Types].xml", "_rels", "docProps", "word"], { cwd: dir });
}
const types = (extra: string) => `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/><Override PartName="/word/numbering.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"/><Override PartName="/word/settings.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml"/><Override PartName="/word/fontTable.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.fontTable+xml"/><Override PartName="/word/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/><Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/>${extra}</Types>`;
const rootRels = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="${W}/officeDocument" Target="word/document.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/></Relationships>`;
const docRels = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="${W}/styles" Target="styles.xml"/><Relationship Id="rId2" Type="${W}/numbering" Target="numbering.xml"/><Relationship Id="rId3" Type="${W}/settings" Target="settings.xml"/><Relationship Id="rId4" Type="${W}/fontTable" Target="fontTable.xml"/><Relationship Id="rId5" Type="${W}/theme" Target="theme/theme1.xml"/></Relationships>`;
const core = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><dc:title>Resume</dc:title></cp:coreProperties>`;
const settings = (s: string) => s.replace(/<w:rsids>[\s\S]*?<\/w:rsids>/, "").replace(/<w:attachedTemplate[^>]*\/>/, "");
const doc = (src: string, body: string) => `${head(src)}<w:body>${body}</w:body></w:document>`;

// ---------- English: keyreply-resume-v5's look ----------
{
  const src = read("en/word/document.xml");
  const ps = paragraphsOf(src);
  // name, contact, section title, Summary, blank, entry heading, its second line, a point, education heading
  const pick = [[0, ["YOUR NAME"]], [1, ["City · you@example.com · +00 0000 0000 · github.com/you"]], [2, ["profile summary"]], [3, ["One paragraph about who you are and what you build."]], [4, [" "]], [21, ["Role", "Mon YYYY - Present"]], [22, ["Project or Company, City"]], [23, ["Did something, and what came of it."]], [12, ["University", "Mon YYYY - Present"]]];
  const body = pick.map(([i, cols]) => sanitize(ps[i], cols)).join("") + sectPrOf(src).replace(/ w:rsid\w*="[^"]*"/g, "");
  pack("out-en", path.join(outDir, "resume-en.docx"), {
    "[Content_Types].xml": types(""), "_rels/.rels": rootRels, "docProps/core.xml": core, "word/_rels/document.xml.rels": docRels,
    "word/document.xml": doc(src, body), "word/styles.xml": read("en/word/styles.xml"), "word/numbering.xml": read("en/word/numbering.xml"),
    "word/settings.xml": settings(read("en/word/settings.xml")), "word/fontTable.xml": read("en/word/fontTable.xml"), "word/theme/theme1.xml": read("en/word/theme/theme1.xml"),
  });
}

// ---------- Chinese: seek-it-cn-template, tidied (A4, 等线, aligned columns, a navy rule under section titles) ----------
{
  const src = read("cn/word/document.xml");
  const ps = paragraphsOf(src);
  const width = 11906 - 2 * 794; // A4 less the margins
  const numId = ps[16].match(/<w:numId w:val="(\d+)"\/>/)[1];
  const grey = `<w:color w:val="595959"/>`;
  const body = [
    `<w:p><w:pPr><w:pStyle w:val="Title"/></w:pPr><w:r><w:t>[姓名]</w:t></w:r></w:p>`,
    `<w:p><w:pPr><w:pStyle w:val="Subtitle"/></w:pPr><w:r><w:t xml:space="preserve">[联系电话]  ·  [电子邮箱]  ·  [所在城市]</w:t></w:r></w:p>`,
    `<w:p><w:pPr><w:pStyle w:val="Subtitle"/></w:pPr><w:r><w:t>GitHub：[主页地址]</w:t></w:r></w:p>`,
    `<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>项目经历</w:t></w:r></w:p>`,
    `<w:p><w:pPr><w:pStyle w:val="ResumeExperienceHeader"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t>[项目或单位名称]</w:t></w:r><w:r><w:rPr><w:b w:val="0"/></w:rPr><w:tab/><w:t>[本人角色]</w:t></w:r><w:r><w:rPr><w:b w:val="0"/>${grey}</w:rPr><w:tab/><w:t>20XX.XX - 20XX.XX</w:t></w:r></w:p>`,
    `<w:p><w:pPr><w:pStyle w:val="ResumeEntry"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">[学校名称]  ·  [专业名称]</w:t></w:r><w:r><w:rPr><w:b w:val="0"/>${grey}</w:rPr><w:tab/><w:t>20XX.XX - 20XX.XX</w:t></w:r></w:p>`,
    `<w:p><w:pPr><w:pStyle w:val="ResumeDetail"/></w:pPr><w:r><w:t>[一句话说明项目解决的问题]</w:t></w:r></w:p>`,
    `<w:p><w:pPr><w:pStyle w:val="ResumeBullet"/><w:keepNext w:val="0"/><w:numPr><w:ilvl w:val="0"/><w:numId w:val="${numId}"/></w:numPr><w:ind w:left="220" w:hanging="220"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t>功能实现：</w:t></w:r><w:r><w:t>围绕[业务场景]开发[核心功能]，交付[具体模块]</w:t></w:r></w:p>`,
  ].join("") + `<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="720" w:right="794" w:bottom="720" w:left="794" w:header="283" w:footer="283" w:gutter="0"/><w:cols w:space="720"/><w:docGrid w:linePitch="360"/></w:sectPr>`;
  // styles: 等线 everywhere; sizes, colours, spacing and tab stops for A4
  let st = read("cn/word/styles.xml").replace(/ w:rsid\w*="[^"]*"/g, "");
  st = st.replace(/<w:rFonts [^>]*\/>/g, `<w:rFonts w:ascii="DengXian" w:hAnsi="DengXian" w:eastAsia="DengXian" w:cs="DengXian"/>`);
  const style = (id, f) => { const re = new RegExp(`(<w:style [^>]*w:styleId="${id}"[^>]*>)([\\s\\S]*?)(</w:style>)`); if (!re.test(st)) throw new Error(id); st = st.replace(re, (_, a, b, c) => a + f(b) + c); };
  const ppr = (b, x) => b.replace(/<w:pPr>[\s\S]*?<\/w:pPr>/, `<w:pPr>${x}</w:pPr>`).replace(/^(?![\s\S]*<w:pPr>)/, "");
  const rpr = (b, x) => (/<w:rPr>/.test(b) ? b.replace(/<w:rPr>[\s\S]*?<\/w:rPr>/, `<w:rPr>${x}</w:rPr>`) : b + `<w:rPr>${x}</w:rPr>`);
  const font = `<w:rFonts w:ascii="DengXian" w:hAnsi="DengXian" w:eastAsia="DengXian" w:cs="DengXian"/>`;
  style("Normal", (b) => rpr(ppr(b, `<w:widowControl/><w:spacing w:before="0" w:after="20" w:line="300" w:lineRule="exact"/>`), `${font}<w:color w:val="262626"/><w:sz w:val="21"/><w:szCs w:val="21"/>`));
  style("Title", (b) => rpr(ppr(b, `<w:keepNext/><w:keepLines/><w:widowControl/><w:spacing w:before="0" w:after="60" w:line="560" w:lineRule="exact"/><w:contextualSpacing/><w:jc w:val="center"/>`), `${font}<w:b/><w:bCs/><w:color w:val="1A1A1A"/><w:spacing w:val="40"/><w:sz w:val="44"/><w:szCs w:val="44"/>`));
  style("Subtitle", (b) => rpr(ppr(b, `<w:keepNext/><w:keepLines/><w:widowControl/><w:spacing w:before="0" w:after="20" w:line="280" w:lineRule="exact"/><w:jc w:val="center"/>`), `${font}<w:i w:val="0"/><w:color w:val="595959"/><w:sz w:val="19"/><w:szCs w:val="19"/>`));
  style("Heading1", (b) => rpr(ppr(b, `<w:keepNext/><w:keepLines/><w:widowControl/><w:pBdr><w:bottom w:val="single" w:sz="6" w:space="2" w:color="1F3A5F"/></w:pBdr><w:spacing w:before="200" w:after="80" w:line="320" w:lineRule="exact"/><w:outlineLvl w:val="0"/>`), `${font}<w:b/><w:bCs/><w:color w:val="1F3A5F"/><w:spacing w:val="10"/><w:sz w:val="23"/><w:szCs w:val="23"/>`));
  style("ResumeEntry", (b) => rpr(ppr(b, `<w:keepNext/><w:keepLines/><w:widowControl/><w:tabs><w:tab w:val="right" w:pos="${width}"/></w:tabs><w:spacing w:before="100" w:after="20" w:line="300" w:lineRule="exact"/>`), `${font}<w:b/><w:color w:val="1A1A1A"/><w:sz w:val="21"/>`));
  style("ResumeExperienceHeader", (b) => rpr(ppr(b, `<w:keepNext/><w:keepLines/><w:widowControl/><w:tabs><w:tab w:val="left" w:pos="5400"/><w:tab w:val="right" w:pos="${width}"/></w:tabs><w:spacing w:before="100" w:after="20" w:line="300" w:lineRule="exact"/><w:ind w:left="0" w:firstLine="0"/>`), `${font}<w:color w:val="1A1A1A"/><w:sz w:val="21"/>`));
  style("ResumeDetail", (b) => rpr(ppr(b, `<w:keepNext/><w:keepLines/><w:widowControl/><w:spacing w:before="0" w:after="30" w:line="290" w:lineRule="exact"/>`), `${font}<w:color w:val="595959"/><w:sz w:val="20"/>`));
  for (const id of ["ResumeBullet", "ResumeSkill"]) style(id, (b) => rpr(ppr(b, `<w:keepLines/><w:widowControl/><w:tabs><w:tab w:val="left" w:pos="220"/></w:tabs><w:spacing w:before="0" w:after="20" w:line="300" w:lineRule="exact"/><w:ind w:left="220" w:hanging="220"/>`), `${font}<w:color w:val="262626"/><w:sz w:val="21"/>`));
  pack("out-cn", path.join(outDir, "resume-cn.docx"), {
    "[Content_Types].xml": types(""), "_rels/.rels": rootRels, "docProps/core.xml": core, "word/_rels/document.xml.rels": docRels,
    "word/document.xml": doc(src, body), "word/styles.xml": st, "word/numbering.xml": read("cn/word/numbering.xml").replace(/ w:rsid\w*="[^"]*"/g, ""),
    "word/settings.xml": settings(read("cn/word/settings.xml")), "word/fontTable.xml": read("cn/word/fontTable.xml"), "word/theme/theme1.xml": read("cn/word/theme/theme1.xml"),
  });
}
fs.rmSync(work, { recursive: true, force: true });
console.log(`wrote ${outDir}/resume-cn.docx, resume-en.docx`);
