// 站点: 话题与类别 (the stable ID fixed once saved; 中文名、英文名; how many works use each; removing one that
// is used asks first — the site drops it from those works) and 外部链接 (图标、名称、链接), each saved with
// its own 保存.
import { useEffect, useState, type ReactNode } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useConfirm } from "@/shared/feedback";
import { Button, TextInput } from "@/shared/ui";
import { SITE_KEY, pageApi, type Link, type SiteData, type TermIn } from "./api";
import { AddRow, RowControls, SizedInput, moved, replaced } from "./parts";

const DATA_KEY = [...SITE_KEY, "terms"];
const small = "py-1 text-sm";
const ICONS = ["github", "linkedin", "email"];

function Card(props: { title: string; dirty: boolean; saving: boolean; onSave: () => void; children: ReactNode }) {
  return (
    <section className="space-y-2">
      <div className="flex items-end justify-between gap-4">
        <h2 className="text-sm text-muted-foreground">{props.title}</h2>
        <Button variant="primary" disabled={!props.dirty || props.saving} onClick={props.onSave}>
          {props.saving ? "正在保存…" : "保存"}
        </Button>
      </div>
      <div className="rounded-card border bg-card p-4">{props.children}</div>
    </section>
  );
}

function TermRows(props: { label: string; rows: TermIn[]; known: Set<string>; set: (rows: TermIn[]) => void }) {
  const [confirm, confirmDialog] = useConfirm();
  const remove = async (i: number) => {
    const t = props.rows[i];
    if (t.used && !(await confirm({ title: `删除${props.label}「${t.zh || t.slug}」？`, message: `${t.used} 篇作品用着它，会变成没有这个${props.label}。`, confirmLabel: "删除", danger: true }))) return;
    props.set(props.rows.filter((_, j) => j !== i));
  };
  return (
    <div className="space-y-1.5">
      <p className="text-xs text-muted-foreground">{props.label}</p>
      {props.rows.map((t, i) => (
        <div key={i} className="flex items-center gap-2">
          <SizedInput aria-label="ID" placeholder="ID" value={t.slug} readOnly={props.known.has(t.slug)} onChange={(e) => props.set(replaced(props.rows, i, { slug: e.target.value.toLowerCase() }))} className={`w-40 shrink-0 font-mono ${small} ${props.known.has(t.slug) ? "text-muted-foreground" : ""}`} />
          <TextInput aria-label="中文名" placeholder="中文名" value={t.zh} onChange={(e) => props.set(replaced(props.rows, i, { zh: e.target.value }))} className={`min-w-0 flex-1 ${small}`} />
          <TextInput aria-label="英文名" placeholder="英文名" value={t.en} onChange={(e) => props.set(replaced(props.rows, i, { en: e.target.value }))} className={`min-w-0 flex-1 ${small}`} />
          <span className="w-12 shrink-0 text-right text-xs text-muted-foreground tabular-nums">{t.used ? `${t.used} 篇` : ""}</span>
          <RowControls i={i} n={props.rows.length} onMove={(to) => props.set(moved(props.rows, i, to))} onRemove={() => void remove(i)} />
        </div>
      ))}
      <AddRow onClick={() => props.set([...props.rows, { slug: "", zh: "", en: "", used: 0 }])}>新{props.label}</AddRow>
      {confirmDialog}
    </div>
  );
}

function LinkRows(props: { rows: Link[]; set: (rows: Link[]) => void }) {
  return (
    <div className="space-y-1.5">
      {props.rows.map((l, i) => (
        <div key={i} className="flex items-center gap-2">
          <SizedInput aria-label="图标" placeholder="图标" list="site-link-icons" value={l.key} onChange={(e) => props.set(replaced(props.rows, i, { key: e.target.value }))} className={`w-28 shrink-0 font-mono ${small}`} />
          <SizedInput aria-label="名称" placeholder="名称" value={l.label} onChange={(e) => props.set(replaced(props.rows, i, { label: e.target.value }))} className={`w-36 shrink-0 ${small}`} />
          <TextInput aria-label="链接" placeholder="链接" value={l.href} onChange={(e) => props.set(replaced(props.rows, i, { href: e.target.value }))} className={`min-w-0 flex-1 ${small}`} />
          <RowControls i={i} n={props.rows.length} onMove={(to) => props.set(moved(props.rows, i, to))} onRemove={() => props.set(props.rows.filter((_, j) => j !== i))} />
        </div>
      ))}
      <AddRow onClick={() => props.set([...props.rows, { key: "", label: "", href: "" }])}>新链接</AddRow>
      <datalist id="site-link-icons">
        {ICONS.map((k) => (
          <option key={k} value={k} />
        ))}
      </datalist>
    </div>
  );
}

export function SiteDataPage(props: { setDirty: (d: boolean) => void; onError: (e: unknown) => void; onChanged: () => void }) {
  const qc = useQueryClient();
  const { data } = useQuery({ queryKey: DATA_KEY, queryFn: pageApi.terms, gcTime: 0 });
  const [terms, setTerms] = useState<SiteData["terms"] | null>(null);
  const [links, setLinks] = useState<Link[] | null>(null);
  const [saving, setSaving] = useState<"terms" | "links" | null>(null);
  useEffect(() => {
    if (data) {
      setTerms(data.terms);
      setLinks(data.links);
    }
  }, [data]);
  const termsDirty = !!data && !!terms && JSON.stringify(terms) !== JSON.stringify(data.terms);
  const linksDirty = !!data && !!links && JSON.stringify(links) !== JSON.stringify(data.links);
  useEffect(() => props.setDirty(termsDirty || linksDirty), [termsDirty, linksDirty]); // eslint-disable-line react-hooks/exhaustive-deps
  if (!data || !terms || !links) return null;
  const known = new Set([...data.terms.categories, ...data.terms.topics].map((t) => t.slug));
  const save = async (what: "terms" | "links") => {
    setSaving(what);
    try {
      const next = what === "terms" ? await pageApi.termsSave(terms, data.termsPrint) : await pageApi.linksSave(links, data.linksPrint);
      qc.setQueryData(DATA_KEY, next);
      void qc.invalidateQueries({ queryKey: [...SITE_KEY, "works"] });
      props.onChanged();
    } catch (e) {
      props.onError(e);
    } finally {
      setSaving(null);
    }
  };
  return (
    <div className="-m-1 min-h-0 flex-1 overflow-y-auto border-t p-1">
      <div className="mx-auto max-w-3xl space-y-6 px-8 py-5">
        <Card title="话题与类别" dirty={termsDirty} saving={saving === "terms"} onSave={() => void save("terms")}>
          <div className="space-y-5">
            <TermRows label="类别" rows={terms.categories} known={known} set={(categories) => setTerms({ ...terms, categories })} />
            <TermRows label="话题" rows={terms.topics} known={known} set={(topics) => setTerms({ ...terms, topics })} />
          </div>
        </Card>
        <Card title="外部链接" dirty={linksDirty} saving={saving === "links"} onSave={() => void save("links")}>
          <LinkRows rows={links} set={setLinks} />
        </Card>
      </div>
    </div>
  );
}
