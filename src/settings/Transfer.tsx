// 导出 / 导入 rows: one Pomelo data file (.json) for all modules. Rules live in Rust (transfer.rs).
import { useQueryClient } from "@tanstack/react-query";
import { open, save } from "@tauri-apps/plugin-dialog";
import { FileInput, FileOutput } from "lucide-react";
import { moduleLabel } from "@/app/modules";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { api } from "@/shared/ipc";
import { Button } from "@/shared/ui";
import { Row } from "./layout";

const FILTERS = [{ name: "Pomelo 数据文件", extensions: ["json"] }];

const today = () => {
  const d = new Date();
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
};

export function ExportRow() {
  const toast = useToast();
  const run = async () => {
    const path = await save({ defaultPath: `pomelo-export-${today()}.json`, filters: FILTERS });
    if (!path) return;
    try {
      const r = await api.exportTo(path);
      toast(`已导出：${r.modules.map((m) => `${m.label} ${m.incoming} 条`).join("，")}`);
    } catch (e) {
      toast(errorText(e), "error");
    }
  };
  return (
    <Row
      label="导出"
      help="把所有模块存成一个 Pomelo 数据文件（.json），适合长期保存、换电脑，也能再导入。"
      actions={
        <Button onClick={() => void run()}>
          <FileOutput size={14} strokeWidth={1.5} /> 导出…
        </Button>
      }
    />
  );
}

export function ImportRow() {
  const qc = useQueryClient();
  const toast = useToast();
  const [confirm, confirmDialog] = useConfirm();

  const run = async () => {
    const path = await open({ multiple: false, directory: false, filters: FILTERS });
    if (!path) return;
    try {
      const p = await api.importPreview(path);
      const overwrite = p.modules.some((m) => m.existing > 0);
      const ok = await confirm({
        title: overwrite ? "导入并覆盖现有数据？" : "导入这些数据？",
        confirmLabel: overwrite ? "覆盖导入" : "导入",
        danger: overwrite,
        message: (
          <ul className="space-y-1">
            {p.modules.map((m) => (
              <li key={m.key}>
                {m.label}：文件里有 {m.incoming} 条
                {m.existing > 0 && <span className="text-destructive">，会替换现有的 {m.existing} 条</span>}
              </li>
            ))}
            {p.skipped.length > 0 && <li>暂不支持，会跳过：{p.skipped.map(moduleLabel).join("、")}</li>}
            {overwrite && <li>覆盖前会自动备份现有数据。</li>}
          </ul>
        ),
      });
      if (!ok) return;
      const r = await api.importApply(path, overwrite);
      await qc.invalidateQueries();
      toast(`已导入：${r.modules.map((m) => `${m.label} ${m.incoming} 条`).join("，")}`);
    } catch (e) {
      toast(errorText(e), "error");
    }
  };

  return (
    <Row
      label="导入"
      help="从 Pomelo 数据文件导入，比如导出的文件、由 Excel 或 Markdown 待办转换来的文件。文件里有的模块会被替换，替换前会先确认并备份。"
      actions={
        <Button onClick={() => void run()}>
          <FileInput size={14} strokeWidth={1.5} /> 选择文件…
        </Button>
      }
    >
      {confirmDialog}
    </Row>
  );
}
