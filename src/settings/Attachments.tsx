// 设置 → 数据 → 附件: how much the copies take; copies no note / task / goal uses can be cleaned up.
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ATTACHMENTS_KEY, attachmentsApi, fileSize } from "@/shared/attachments/api";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { Button } from "@/shared/ui";
import { Row } from "./layout";

export function AttachmentsRow() {
  const qc = useQueryClient();
  const toast = useToast();
  const [confirm, confirmDialog] = useConfirm();
  const usage = useQuery({ queryKey: [...ATTACHMENTS_KEY, "usage"], queryFn: attachmentsApi.usage }).data;
  if (!usage) return null;
  const clean = async () => {
    const ok = await confirm({
      title: `删除 ${usage.unused} 个没用到的附件？`,
      message: `共 ${fileSize(usage.unusedBytes)}。它们是复制进来的那一份，你原来的文件不受影响；删除后不能撤销。`,
      confirmLabel: "删除",
      danger: true,
    });
    if (!ok) return;
    try {
      const n = await attachmentsApi.clean();
      toast(`已删除 ${n} 个附件`);
      void qc.invalidateQueries({ queryKey: ATTACHMENTS_KEY });
    } catch (e) {
      toast(errorText(e), "error");
    }
  };
  return (
    <Row
      label="附件"
      help="笔记、任务和目标里的附件（复制进来的一份，在数据目录的 attachments 文件夹）。小拾的附件和资料另外放，算在「小拾 → 空间」里，两边不重复。不进每日备份。删掉笔记或任务后，附件先留着，在这里清理。"
      actions={usage.unused > 0 ? <Button onClick={() => void clean()}>清理没用到的</Button> : undefined}
    >
      {usage.files} 个 · {fileSize(usage.bytes)}
      {usage.unused > 0 && <span className="ml-2 text-xs text-muted-foreground">其中 {usage.unused} 个没用到（{fileSize(usage.unusedBytes)}）</span>}
      {confirmDialog}
    </Row>
  );
}
