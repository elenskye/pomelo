import type { ReactNode } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { open } from "@tauri-apps/plugin-dialog";
import { FolderOpen, Settings, Upload } from "lucide-react";
import { moduleLabel } from "@/app/modules";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { api } from "@/shared/ipc";
import { Button, PageHeader } from "@/shared/ui";

function Row(props: { label: string; children: ReactNode }) {
  return (
    <div className="flex items-center gap-6 border-b px-4 py-3 last:border-b-0">
      <div className="w-24 shrink-0 text-muted-foreground">{props.label}</div>
      <div className="min-w-0 flex-1">{props.children}</div>
    </div>
  );
}

function Section(props: { title: string; children: ReactNode }) {
  return (
    <section className="mb-6">
      <h2 className="mb-2 px-1 text-sm text-muted-foreground">{props.title}</h2>
      <div className="rounded-card border bg-card">{props.children}</div>
    </section>
  );
}

const Path = ({ value }: { value: string }) => (
  <span className="selectable break-all font-mono text-xs">{value}</span>
);

function ImportRow() {
  const qc = useQueryClient();
  const toast = useToast();
  const [confirm, confirmDialog] = useConfirm();

  const run = async () => {
    const path = await open({ multiple: false, directory: false, filters: [{ name: "Pomelo 数据文件", extensions: ["json"] }] });
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
    <Row label="导入">
      <div className="flex items-center justify-between gap-4">
        <span className="text-sm text-muted-foreground">从 Pomelo 数据文件（.json）导入，比如由 Excel 原型转换来的文件</span>
        <Button onClick={() => void run()}>
          <Upload size={14} strokeWidth={1.5} /> 选择文件…
        </Button>
      </div>
      {confirmDialog}
    </Row>
  );
}

export default function SettingsPage() {
  const { data: info } = useQuery({ queryKey: ["app_info"], queryFn: api.appInfo });
  if (!info) return null;
  return (
    <div className="flex h-full flex-col overflow-y-auto">
      <PageHeader icon={Settings} color="var(--muted-foreground)" title="设置" />
      <div className="max-w-3xl px-8 py-4">
        <Section title="数据">
          <Row label="数据目录">
            <div className="flex items-center justify-between gap-4">
              <Path value={info.dataDir} />
              <Button onClick={() => void api.revealDataDir()}>
                <FolderOpen size={14} strokeWidth={1.5} /> 在 Finder 中打开
              </Button>
            </div>
          </Row>
          <Row label="数据库">
            <Path value={info.dbPath} />
          </Row>
          <Row label="数据库版本">{info.schemaVersion ?? "—"}</Row>
          <ImportRow />
        </Section>
        <Section title="关于">
          <Row label="版本">{info.appVersion}</Row>
          <Row label="运行方式">
            {info.isDev ? "开发版：使用测试数据，和正式版的数据完全分开" : "正式版"}
          </Row>
        </Section>
      </div>
    </div>
  );
}
