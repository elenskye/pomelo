// 设置: five pages on the paper's folder tabs (纸签) — 通用 (外观、启动、通知、关于), 数据 (数据、导出与导入、备份), 柚子,
// 模块 (呦呦漫游、芝麻开门) and 体检 (Health.tsx). A bare /settings shows the one open last.
import { useQuery } from "@tanstack/react-query";
import { FolderOpen, Settings } from "@/shared/icons";
import { useSubNav, useSubPage } from "@/app/subpage";
import { api } from "@/shared/ipc";
import { Button, PageHeader } from "@/shared/ui";
import { AppearanceSection } from "./Appearance";
import { AttachmentsRow } from "./Attachments";
import { BackupsSection } from "./Backups";
import HealthPage from "./Health";
import { YouziSection } from "./Youzi";
import { Path, Row, Section } from "./layout";
import { NotifySection } from "./Notify";
import { SiteSection } from "./Site";
import { StartupSection } from "./Startup";
import { ExportRow, ImportRow } from "./Transfer";
import { VaultSection } from "./Vault";

type Page = "general" | "data" | "youzi" | "modules" | "health";

export default function SettingsPage() {
  const [page, setPage] = useSubPage<Page>("settings");
  useSubNav("settings", (p) => setPage(p as Page));
  const { data: info } = useQuery({ queryKey: ["app_info"], queryFn: api.appInfo });
  if (page === "health") return <HealthPage />;
  if (!info) return null;
  return (
    <div className="flex h-full flex-col overflow-y-auto">
      <PageHeader icon={Settings} color="var(--muted-foreground)" title="设置" />
      <div className="px-8 py-4">
        {page === "general" && (
          <>
            <AppearanceSection />
            <StartupSection />
            <NotifySection />
            <Section title="关于">
              <Row label="版本">{info.appVersion}</Row>
              <Row label="运行方式" help={info.isDev ? "开发版使用测试数据，和正式版的数据完全分开。" : undefined}>
                {info.isDev ? "开发版" : "正式版"}
              </Row>
              <Row label="撤销" help="⌘Z 撤销、⌘⇧Z 重做最近的修改（新增、修改、删除、勾选、排序都算）。退出 App 后就不能撤销了；关窗口不算退出。">
                ⌘Z 撤销 · ⌘⇧Z 重做
              </Row>
            </Section>
          </>
        )}
        {page === "data" && (
          <>
        <Section title="数据">
          <Row
            label="数据目录"
            help="所有数据都在这个文件夹里：数据库、附件和备份。不要把它放进 iCloud 这类同步文件夹。"
            actions={
              <Button onClick={() => void api.revealDataDir()}>
                <FolderOpen size={14} strokeWidth={1.5} /> 在 Finder 中打开
              </Button>
            }
          >
            <Path value={info.dataDir} />
          </Row>
          <Row label="数据库">
            <Path value={info.dbPath} />
          </Row>
          <Row label="数据库版本" help="每次升级 App、数据结构变了，版本就加一；升级前会自动备份。">
            {info.schemaVersion ?? "—"}
          </Row>
          <AttachmentsRow />
        </Section>
        <Section title="导出与导入">
          <ExportRow />
          <ImportRow />
        </Section>
        <BackupsSection />
          </>
        )}
        {page === "youzi" && <YouziSection />}
        {page === "modules" && (
          <>
            <SiteSection />
            <VaultSection />
          </>
        )}
      </div>
    </div>
  );
}
