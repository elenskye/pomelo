// 备份 section: list (newest first), 立即备份, restore any snapshot. Rules (naming, retention,
// safety copy before restore) live in Rust (backup.rs); this file only renders and confirms.
import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ArchiveRestore, DatabaseBackup, FolderOpen } from "lucide-react";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { api, type BackupInfo } from "@/shared/ipc";
import { Button } from "@/shared/ui";
import { Section } from "./layout";

const COLLAPSED = 8;

export function backupLabel(label: string) {
  const fixed: Record<string, string> = { daily: "每日自动", manual: "手动", "before-import": "导入前", "before-restore": "恢复前" };
  const v = /^before-v(\d+)$/.exec(label);
  return fixed[label] ?? (v ? `升级前（→ 版本 ${v[1]}）` : label);
}

const minutes = (b: BackupInfo) => b.createdAt.slice(0, 16);

const size = (n: number) => (n < 1024 * 1024 ? `${Math.max(1, Math.round(n / 1024))} KB` : `${(n / 1024 / 1024).toFixed(1)} MB`);

export function BackupsSection() {
  const qc = useQueryClient();
  const toast = useToast();
  const [confirm, confirmDialog] = useConfirm();
  const [showAll, setShowAll] = useState(false);
  const [busy, setBusy] = useState(false);
  const { data: backups = [] } = useQuery({ queryKey: ["backups"], queryFn: api.backupsList });

  const act = async (f: () => Promise<void>) => {
    setBusy(true);
    try {
      await f();
    } catch (e) {
      toast(errorText(e), "error");
    } finally {
      setBusy(false);
    }
  };

  const backupNow = () =>
    act(async () => {
      const b = await api.backupNow();
      await qc.invalidateQueries({ queryKey: ["backups"] });
      toast(`已备份（${minutes(b)}）`);
    });

  const restore = async (b: BackupInfo) => {
    const ok = await confirm({
      title: "从这份备份恢复？",
      confirmLabel: "恢复",
      danger: true,
      message: (
        <div className="space-y-1">
          <p>
            现在的全部数据会换成 <span className="text-foreground">{minutes(b)}</span>（{backupLabel(b.label)}）这份备份里的内容。
          </p>
          <p>恢复前会先把现在的数据备份一份，恢复错了可以再从那份恢复回来。</p>
        </div>
      ),
    });
    if (!ok) return;
    await act(async () => {
      await api.backupRestore(b.fileName);
      await qc.invalidateQueries(); // every module's data changed
      toast(`已恢复到 ${minutes(b)} 的备份`);
    });
  };

  const shown = showAll ? backups : backups.slice(0, COLLAPSED);
  return (
    <Section title="备份">
      <div className="flex items-center justify-between gap-4 border-b px-4 py-3">
        <p className="text-sm text-muted-foreground">
          每天第一次打开时自动备份。每日备份保留最近 30 份，更早的每月留一份；其他备份不会自动删除。
        </p>
        <div className="flex shrink-0 gap-2">
          <Button onClick={() => void api.revealBackupsDir().catch((e) => toast(errorText(e), "error"))}>
            <FolderOpen size={14} strokeWidth={1.5} /> 打开文件夹
          </Button>
          <Button variant="primary" disabled={busy} onClick={() => void backupNow()}>
            <DatabaseBackup size={14} strokeWidth={1.5} /> 立即备份
          </Button>
        </div>
      </div>
      {backups.length === 0 ? (
        <p className="px-4 py-6 text-center text-sm text-muted-foreground">还没有备份</p>
      ) : (
        <ul>
          {shown.map((b) => (
            <li key={b.fileName} className="group flex items-center gap-4 border-b px-4 py-2 last:border-b-0" title={b.fileName}>
              <span className="w-36 shrink-0 tabular-nums">{minutes(b)}</span>
              <span className="min-w-0 flex-1 truncate text-sm text-muted-foreground">{backupLabel(b.label)}</span>
              <span className="w-16 shrink-0 text-right text-xs text-muted-foreground tabular-nums">{size(b.size)}</span>
              <Button variant="ghost" disabled={busy} className="py-1" onClick={() => void restore(b)}>
                <ArchiveRestore size={14} strokeWidth={1.5} /> 恢复
              </Button>
            </li>
          ))}
          {backups.length > COLLAPSED && (
            <li className="px-4 py-2 text-center">
              <button type="button" className="text-sm text-primary hover:underline" onClick={() => setShowAll((v) => !v)}>
                {showAll ? "收起" : `显示全部 ${backups.length} 份`}
              </button>
            </li>
          )}
        </ul>
      )}
      {confirmDialog}
    </Section>
  );
}
