// 资源库: every image and document the site's content uses (public/assets/…, each registered). 图片 · 文档 with
// their sizes, a 没在用 mark on those nothing refers to; 添加… (or dropped files) adds to the library (big images
// shrunk); a click opens the panel (Alt / 文档说明 with 润色, 用途); 删除 first lists where it is used (the site's own
// scan), then asks (an unused file may go too). Files put in the folder by hand: 登记 registers them.
import { useRef, useState, type FormEvent } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { FileText, Plus } from "lucide-react";
import { Polish } from "@/modules/picky/Assist";
import { useDropTarget } from "@/shared/attachments/target";
import { errorText } from "@/shared/feedback";
import { Modal } from "@/shared/Modal";
import { SidePanel } from "@/shared/panel";
import { Button, EmptyState, Field, Segmented, TextArea, TextInput } from "@/shared/ui";
import { SITE_KEY, pageApi, siteSrc, sizeText, type AssetKind, type Citation, type RegEntry } from "./api";
import { addTo, pickFiles } from "./parts";

const REG_KEY = [...SITE_KEY, "registry"];
const UNUSED = "shrink-0 rounded-full bg-muted px-1.5 text-[11px] text-muted-foreground";

function DeleteDialog(props: { entry: RegEntry; onClose: () => void; onDeleted: () => void }) {
  const e = props.entry;
  const { data: cites } = useQuery({ queryKey: [...REG_KEY, "cites", e.kind, e.slug], queryFn: () => pageApi.citations(e.kind, e.slug), gcTime: 0 });
  const [withFile, setWithFile] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const submit = async (ev: FormEvent) => {
    ev.preventDefault();
    try {
      await pageApi.registryDelete(e.kind, e.slug, e.print, withFile && !!cites && cites.length === 0);
      props.onDeleted();
    } catch (err) {
      setError(errorText(err));
    }
  };
  return (
    <Modal title={`删除登记「${e.slug}」？`} wide onClose={props.onClose} onSubmit={(ev) => void submit(ev)} error={error} submitLabel="删除" submitDisabled={!cites}>
      {!cites ? (
        <p className="text-sm text-muted-foreground">正在找哪里在用…</p>
      ) : cites.length ? (
        <div className="space-y-1.5">
          <p className="text-sm">还在这些地方用着：</p>
          <ul className="max-h-48 overflow-y-auto rounded-control border px-3 py-2 font-mono text-xs leading-relaxed">
            {cites.map((c: Citation) => (
              <li key={`${c.path}-${c.role}`}>
                {c.path}
                {c.role && <span className="text-muted-foreground"> · {c.role === "Cover" ? "封面" : "正文"}</span>}
              </li>
            ))}
          </ul>
          <p className="text-xs text-muted-foreground">只删登记，文件留着。</p>
        </div>
      ) : (
        <label className="flex items-center gap-2 text-sm">
          <input type="checkbox" checked={withFile} onChange={(ev) => setWithFile(ev.target.checked)} />
          没有地方在用，文件 <span className="font-mono text-xs">{e.file}</span> 也删掉
        </label>
      )}
    </Modal>
  );
}

function EntryPanel(props: { entry: RegEntry; onClose: () => void; onSaved: (e: RegEntry) => void; onDelete: () => void }) {
  const [e, setE] = useState(props.entry);
  const [error, setError] = useState<string | null>(null);
  const submit = async (ev: FormEvent) => {
    ev.preventDefault();
    try {
      props.onSaved(await pageApi.registrySave(e));
    } catch (err) {
      setError(errorText(err));
    }
  };
  return (
    <SidePanel title={e.slug} onClose={props.onClose} onSubmit={(ev) => void submit(ev)} onDelete={props.onDelete} error={error}>
      {e.kind === "image" ? <img src={siteSrc(e.file)} alt={e.note} className="max-h-56 w-full rounded-control border bg-muted object-contain" /> : null}
      <Field label="文件">
        <span className="selectable font-mono text-xs break-all">{e.file}</span>
        <span className="mt-1 flex items-center gap-2 text-xs text-muted-foreground">
          {sizeText(e.size)}
          {!e.used && <span className={UNUSED}>没在用</span>}
        </span>
      </Field>
      <Field label={e.kind === "image" ? "图片描述（Alt）" : "文档说明"}>
        <Polish module="site" value={e.note} onChange={(note) => setE({ ...e, note })}>
          <TextArea rows={3} value={e.note} onChange={(x) => setE({ ...e, note: x.target.value })} />
        </Polish>
      </Field>
      <Field label="用途">
        <TextInput value={e.usage} onChange={(x) => setE({ ...e, usage: x.target.value })} />
      </Field>
    </SidePanel>
  );
}

export function AssetsPage(props: { onError: (e: unknown) => void; onChanged: () => void }) {
  const qc = useQueryClient();
  const { data: all } = useQuery({ queryKey: REG_KEY, queryFn: pageApi.registry, refetchOnWindowFocus: true, refetchOnMount: "always" });
  const [kind, setKind] = useState<AssetKind>("image");
  const { data: loose = [] } = useQuery({ queryKey: [...REG_KEY, "loose", kind], queryFn: () => pageApi.unregistered(kind), refetchOnWindowFocus: true, refetchOnMount: "always" });
  const [open, setOpen] = useState<RegEntry | null>(null);
  const [deleting, setDeleting] = useState<RegEntry | null>(null);
  const box = useRef<HTMLDivElement>(null);
  const list = (all ?? []).filter((e) => e.kind === kind);
  const changed = () => {
    void qc.invalidateQueries({ queryKey: REG_KEY });
    void qc.invalidateQueries({ queryKey: [...SITE_KEY, "assets"] });
    props.onChanged();
  };
  const added = (p: Promise<unknown>) => void p.then(changed, props.onError);
  const take = addTo(`asset:${kind}`);
  const dropping = useDropTarget(box, { paths: (paths) => added(take({ paths })), files: (files) => added(take({ files })) });
  const count = (k: AssetKind) => (all ?? []).filter((e) => e.kind === k).length;
  const total = list.reduce((n, e) => n + e.size, 0);
  return (
    <div ref={box} className={`relative flex min-h-0 flex-1 flex-col border-t ${dropping ? "ring-2 ring-primary ring-inset" : ""}`}>
      <div className="flex shrink-0 items-center justify-between gap-3 px-6 py-2">
        <Segmented
          ariaLabel="类型"
          value={kind}
          onChange={setKind}
          options={[
            { value: "image", label: <>图片 <span className="text-xs text-muted-foreground">{count("image")}</span></> },
            { value: "document", label: <>文档 <span className="text-xs text-muted-foreground">{count("document")}</span></> },
          ]}
        />
        <div className="flex items-center gap-3">
          {all && list.length > 0 && <span className="text-xs text-muted-foreground">共 {sizeText(total)}</span>}
          <Button variant="primary" onClick={() => void pickFiles(true, kind === "image").then((paths) => paths.length && added(take({ paths })))}>
            <Plus size={14} /> 添加…
          </Button>
        </div>
      </div>
      {loose.length > 0 && (
        <p className="mx-6 mb-2 text-xs text-muted-foreground">
          文件夹里还有 {loose.length} 个文件没登记
          <button type="button" className="ml-2 rounded px-1 text-primary outline-none hover:underline focus-visible:ring-2 focus-visible:ring-ring" onClick={() => added(pageApi.adopt(kind))}>
            登记
          </button>
        </p>
      )}
      <div className="-m-1 min-h-0 flex-1 overflow-y-auto p-1">
        {all && list.length === 0 ? (
          <EmptyState title={kind === "image" ? "资源库里还没有图片" : "资源库里还没有文档"} />
        ) : kind === "image" ? (
          <div className="grid grid-cols-[repeat(auto-fill,minmax(150px,1fr))] gap-3 px-6 pb-6">
            {list.map((e) => (
              <button key={e.slug} type="button" onClick={() => setOpen(e)} className={`space-y-1 rounded-control p-1 text-left outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring ${open?.slug === e.slug ? "bg-accent" : ""}`}>
                <img src={siteSrc(e.file)} alt={e.note} className="aspect-[4/3] w-full rounded-control border bg-muted object-cover" />
                <span className="flex items-center gap-1.5">
                  <span className="min-w-0 flex-1 truncate font-mono text-xs">{e.slug}</span>
                  {!e.used && <span className={UNUSED}>没在用</span>}
                </span>
                <span className="flex gap-1.5 text-[11px] text-muted-foreground">
                  <span className="min-w-0 flex-1 truncate">{e.usage}</span>
                  <span className="shrink-0 tabular-nums">{sizeText(e.size)}</span>
                </span>
              </button>
            ))}
          </div>
        ) : (
          <div className="mx-6 mb-6 overflow-hidden rounded-card border bg-card">
            {list.map((e) => (
              <button key={e.slug} type="button" onClick={() => setOpen(e)} className={`flex w-full items-center gap-3 border-b px-4 py-2.5 text-left text-sm last:border-b-0 hover:bg-muted/70 ${open?.slug === e.slug ? "bg-accent" : ""}`}>
                <FileText size={14} className="shrink-0 text-muted-foreground" />
                <span className="font-mono text-xs">{e.slug}</span>
                {!e.used && <span className={UNUSED}>没在用</span>}
                <span className="min-w-0 flex-1 truncate text-xs text-muted-foreground">{e.note || e.usage}</span>
                <span className="shrink-0 text-xs text-muted-foreground tabular-nums">{sizeText(e.size)}</span>
              </button>
            ))}
          </div>
        )}
      </div>
      {open && (
        <EntryPanel
          key={open.slug + open.kind}
          entry={open}
          onClose={() => setOpen(null)}
          onSaved={() => {
            setOpen(null);
            changed();
          }}
          onDelete={() => setDeleting(open)}
        />
      )}
      {deleting && (
        <DeleteDialog
          entry={deleting}
          onClose={() => setDeleting(null)}
          onDeleted={() => {
            setDeleting(null);
            setOpen(null);
            changed();
          }}
        />
      )}
    </div>
  );
}
