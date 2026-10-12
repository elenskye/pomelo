// 设置 → 小拾 → 空间: sizes, deleting stored copies / unknown files, cleaning old empty conversations
// (rules in Rust picky/storage.rs).
import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { HardDrive } from "lucide-react";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { Button } from "@/shared/ui";
import { byteText, fileDetail, pickyApi, type StoredFile } from "@/modules/picky/api";
import { FileBadge, Panel, openFile } from "@/modules/picky/Files";
import { Row } from "./layout";

const STORAGE_KEY = ["picky", "storage"];

function StorageDialog(props: { onClose: () => void }) {
  const qc = useQueryClient();
  const toast = useToast();
  const [confirm, confirmDialog] = useConfirm();
  const u = useQuery({ queryKey: STORAGE_KEY, queryFn: pickyApi.storage }).data;
  const act = (p: Promise<unknown>, done?: string) =>
    void p.then(
      () => {
        void qc.invalidateQueries({ queryKey: ["picky"] });
        if (done) toast(done);
      },
      (e) => toast(errorText(e), "error"),
    );
  const removeFile = async (f: StoredFile) => {
    const ok = await confirm({
      title: `删除「${f.name}」？`,
      confirmLabel: "删除",
      danger: true,
      message: (
        <ul className="space-y-1">
          <li>删除小拾保存的这份副本，原文件不受影响。</li>
          {f.messages > 0 && <li>用到它的 {f.messages} 个问题会显示「文件已删除」。</li>}
          {f.projectFiles > 0 && <li>项目资料里的这一版不能再读取，索引也会删掉。</li>}
          <li>不能撤销。</li>
        </ul>
      ),
    });
    if (ok) act(pickyApi.storageDeleteFile(f.id));
  };
  const removeStray = async (name: string) => {
    if (await confirm({ title: `删除「${name}」？`, message: "这个文件没有任何记录在用。不能撤销。", confirmLabel: "删除", danger: true })) act(pickyApi.storageDeleteStray(name));
  };
  const uses = (f: StoredFile) => [f.messages > 0 && `用于 ${f.messages} 个问题`, f.projectFiles > 0 && "项目资料"].filter(Boolean).join(" · ") || "没有用到";
  const head = "px-2 pt-3 pb-1 text-xs text-muted-foreground";
  return (
    <Panel title="空间管理" onClose={props.onClose}>
      {u && (
        <div className="-m-1 min-h-40 overflow-y-auto p-1">
          <div className="px-3 pb-3">
            <p className={head}>
              数据库 {byteText(u.dbBytes)} · 附件和资料 {byteText(u.filesBytes)}
            </p>
            <div className="flex items-center gap-3 rounded-control px-2 py-2">
              <p className="min-w-0 flex-1 text-sm">30 天没动过的空对话 {u.emptySessions} 个</p>
              <Button className="py-1" disabled={!u.emptySessions} onClick={() => act(pickyApi.storageCleanEmpty(), `已清理 ${u.emptySessions} 个空对话`)}>
                清理
              </Button>
            </div>
            <p className={head}>附件和资料</p>
            {!u.files.length && <p className="px-2 py-4 text-center text-sm text-muted-foreground">暂无文件</p>}
            {u.files.map((f) => (
              <div key={f.id} className="flex items-center gap-3 rounded-control px-2 py-2 hover:bg-muted/50">
                <FileBadge format={f.format} />
                <div className="min-w-0 flex-1">
                  <p className="truncate text-sm">{f.name}</p>
                  <p className="text-xs text-muted-foreground">
                    {f.bytes ? byteText(f.bytes) : fileDetail(f)} · {uses(f)}
                  </p>
                </div>
                <Button className="py-1" onClick={() => openFile(f.id, toast)}>
                  打开
                </Button>
                <Button className="py-1" variant="ghost-danger" onClick={() => void removeFile(f)}>
                  删除
                </Button>
              </div>
            ))}
            {u.strays.length > 0 && (
              <>
                <p className={head}>没有记录的文件</p>
                {u.strays.map((s) => (
                  <div key={s.name} className="flex items-center gap-3 rounded-control px-2 py-2 hover:bg-muted/50">
                    <div className="min-w-0 flex-1">
                      <p className="truncate font-mono text-xs">{s.name}</p>
                      <p className="text-xs text-muted-foreground">{byteText(s.bytes)}</p>
                    </div>
                    <Button className="py-1" variant="ghost-danger" onClick={() => void removeStray(s.name)}>
                      删除
                    </Button>
                  </div>
                ))}
              </>
            )}
          </div>
        </div>
      )}
      {confirmDialog}
    </Panel>
  );
}

export function StorageRow() {
  const [open, setOpen] = useState(false);
  const u = useQuery({ queryKey: STORAGE_KEY, queryFn: pickyApi.storage }).data;
  if (!u) return null;
  return (
    <Row
      label="空间"
      help="小拾的数据库，加上附件和项目资料的副本（备份另算）。可以删掉不要的副本、清理 30 天没动过的空对话。"
      actions={
        <Button onClick={() => setOpen(true)}>
          <HardDrive size={14} strokeWidth={1.5} /> 管理…
        </Button>
      }
    >
      {byteText(u.dbBytes + u.filesBytes)}
      <span className="ml-2 text-xs text-muted-foreground">
        数据库 {byteText(u.dbBytes)} · 附件和资料 {byteText(u.filesBytes)}
      </span>
      {open && <StorageDialog onClose={() => setOpen(false)} />}
    </Row>
  );
}
