import { useState } from "react";
import { Dropdown } from "@/shared/Dropdown";
import { Button, EmptyState, Segmented } from "@/shared/ui";
import { useConfirm } from "@/shared/feedback";
import { SeekerDiff, draftText, recordText, when } from "./SeekerParts";
import { draftLang, seekerApi, type SeekerCatalog, type SeekerDraft, type SeekerLang, type SeekerOutput } from "./seekerApi";
import { SeekerExports } from "./SeekerOutput";
import { SeekerCvHistory } from "./SeekerFiles";
import type { SeekerWrite } from "./SeekerLibrary";

/** The page's language: its exports and versions; 资料修订 of its records (and the shared ones). */
export function SeekerHistory(props: { lang: SeekerLang; output: SeekerOutput; catalog: SeekerCatalog; draft: SeekerDraft | null; busy: boolean; write: SeekerWrite }) {
  const [selected, setSelected] = useState<number | null>(null);
  const [compare, setCompare] = useState<number | null>(null);
  const [part, setPart] = useState<"versions" | "records" | "cv">("versions");
  const records = part === "records";
  const [confirm, dialog] = useConfirm();
  const versions = props.catalog.versions.filter((v) => draftLang(v.body) === props.lang);
  const revisions = props.catalog.revisions.filter((r) => ["fact", "application"].includes(r.body.kind) || r.body.lang === "" || (r.body.lang ?? "cn") === props.lang);
  const current = versions.find((v) => v.id === selected);
  const other = versions.find((v) => v.id === compare);
  return <div className="-m-1 flex-1 space-y-4 overflow-auto p-1 pt-4"><SeekerExports lang={props.lang} output={props.output} draft={props.draft} busy={props.busy} write={props.write} /><Segmented<"versions" | "records" | "cv"> ariaLabel="历史" value={part} onChange={setPart} options={[{ value: "versions", label: "确认版本" }, { value: "records", label: "资料修订" }, { value: "cv", label: "cv/ 里的简历" }]} />
    {part === "cv" ? <SeekerCvHistory lang={props.lang} /> : records ? revisions.map((r) => <article key={r.id} className="space-y-2 rounded-card border bg-card p-4"><strong>{r.body.title}</strong><p className="text-xs text-muted-foreground">{when(r.savedAt)}</p><SeekerDiff before={recordText(r.body)} after={recordText(props.catalog.records.find((v) => v.id === r.recordId))} /><Button disabled={props.busy} onClick={async () => { if (await confirm({ title: "恢复这份资料？", message: "保存为新的当前版本，已有简历不会自动改动。" })) { const existing = props.catalog.records.find((v) => v.id === r.recordId); void props.write(() => seekerApi.recordSave({ ...r.body, revision: existing?.revision ?? 0 })); } }}>恢复为当前版</Button></article>) : <>
      {!versions.length && <EmptyState title="还没有确认版本" />}
      <div className="flex flex-wrap gap-2">{versions.map((v) => <Button key={v.id} variant={selected === v.id ? "primary" : "secondary"} onClick={() => setSelected(v.id)}>{v.label} · {when(v.createdAt)}</Button>)}</div>
      {current && <article className="space-y-3"><div className="flex flex-wrap items-center gap-2"><label className="text-sm">比较<Dropdown<number | null> ariaLabel="比较" value={compare} options={[{ value: null, label: "当前草稿" }, ...versions.filter((v) => v.id !== current.id).map((v) => ({ value: v.id, label: `${v.label} · ${when(v.createdAt)}` }))]} onChange={setCompare} /></label><Button disabled={props.busy} onClick={async () => { if (await confirm({ title: "从此版本另开草稿？", message: "当前草稿会被替换，历史版本保持原样，可以撤销。" })) void props.write(() => seekerApi.restore(current.id)); }}>另开草稿</Button></div><SeekerDiff before={draftText(current.body)} after={draftText(other?.body ?? props.draft)} /><details><summary>查看版本全文</summary><pre className="whitespace-pre-wrap font-sans text-sm">{draftText(current.body)}</pre></details></article>}
    </>}{dialog}</div>;
}
