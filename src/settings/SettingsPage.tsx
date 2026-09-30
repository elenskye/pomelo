import { useQuery } from "@tanstack/react-query";
import { FolderOpen, Settings } from "lucide-react";
import { api } from "@/shared/ipc";
import { Button, PageHeader } from "@/shared/ui";
import { BackupsSection } from "./Backups";
import { Path, Row, Section } from "./layout";
import { ExportRow, ImportRow } from "./Transfer";

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
        </Section>
        <Section title="导出与导入">
          <ExportRow />
          <ImportRow />
        </Section>
        <BackupsSection />
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
