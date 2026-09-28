import type { ReactNode } from "react";
import { useQuery } from "@tanstack/react-query";
import { FolderOpen, Settings } from "lucide-react";
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

export default function SettingsPage() {
  const { data: info } = useQuery({ queryKey: ["app_info"], queryFn: api.appInfo });
  if (!info) return null;
  return (
    <div className="flex h-full flex-col">
      <PageHeader icon={Settings} color="var(--muted-foreground)" title="设置" />
      <div className="max-w-3xl px-8 py-4">
        <Section title="数据">
          <Row label="数据目录">
            <div className="flex items-center justify-between gap-4">
              <Path value={info.dataDir} />
              <Button onClick={() => api.revealDataDir()}>
                <FolderOpen size={14} strokeWidth={1.5} /> 在 Finder 中打开
              </Button>
            </div>
          </Row>
          <Row label="数据库">
            <Path value={info.dbPath} />
          </Row>
          <Row label="数据库版本">{info.schemaVersion ?? "—"}</Row>
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
