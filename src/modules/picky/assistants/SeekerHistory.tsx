import { useState } from "react";
import { Button, Segmented } from "@/shared/ui";
import { useConfirm } from "@/shared/feedback";
import { SeekerDiff, recordText, when } from "./SeekerParts";
import { seekerApi, type SeekerCatalog, type SeekerLang, type SeekerOutput } from "./seekerApi";
import { SeekerExports } from "./SeekerOutput";
import { SeekerCvHistory } from "./SeekerFiles";
import type { SeekerWrite } from "./SeekerLibrary";

/** The page's language: its Word exports (a list; Seeker keeps no copies), cv/'s 简历版本 and the 资料修订 of
 * its records (and the shared ones). */
export function SeekerHistory(props: { lang: SeekerLang; output: SeekerOutput; catalog: SeekerCatalog; busy: boolean; write: SeekerWrite }) {
  const [part, setPart] = useState<"cv" | "records">("cv");
  const [confirm, dialog] = useConfirm();
  const revisions = props.catalog.revisions.filter((r) => ["fact", "application"].includes(r.body.kind) || r.body.lang === "" || (r.body.lang ?? "cn") === props.lang);
  return <div className="-m-1 flex-1 space-y-4 overflow-auto p-1 pt-4">
    <SeekerExports lang={props.lang} output={props.output} />
    <Segmented<"cv" | "records"> ariaLabel="历史" value={part} onChange={setPart} options={[{ value: "cv", label: "简历版本" }, { value: "records", label: "资料修订" }]} />
    {part === "cv" ? <SeekerCvHistory lang={props.lang} /> : revisions.map((r) => <article key={r.id} className="space-y-2 rounded-card border bg-card p-4"><strong>{r.body.title}</strong><p className="text-xs text-muted-foreground">{when(r.savedAt)}</p><SeekerDiff before={recordText(r.body)} after={recordText(props.catalog.records.find((v) => v.id === r.recordId))} /><Button disabled={props.busy} onClick={async () => { if (await confirm({ title: "恢复这份资料？", message: "保存为新的当前版本，已有简历不会自动改动。" })) { const existing = props.catalog.records.find((v) => v.id === r.recordId); void props.write(() => seekerApi.recordSave({ ...r.body, revision: existing?.revision ?? 0 })); } }}>恢复为当前版</Button></article>)}
    {dialog}
  </div>;
}
