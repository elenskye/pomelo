// 呦呦漫游: the personal website. Header: 正式网站 · love-shuyu (the hidden page, online) · 本地预览 (starts the
// site's dev server first) · GitHub · Vercel. Pages: 发布 · 作品 · Now · 关于我 · 职业 · 资源库 · 站点; leaving unsaved edits (another work, another
// page) asks first. A folder that can't be used shows why (开发版 without its copy: 建副本).
import { useRef, useState } from "react";
import { useNavigate } from "react-router";
import { useQueryClient } from "@tanstack/react-query";
import { ExternalLink } from "lucide-react";
import { moduleById } from "@/app/modules";
import { useSubPage } from "@/app/subpage";
import { usePageContext } from "@/app/pageContext";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { api } from "@/shared/ipc";
import { Button, EmptyState, PageHeader, Segmented } from "@/shared/ui";
import { OVERVIEW_KEY, siteApi, useHistory, useOverview, type Overview } from "./api";
import { AboutPage } from "./About";
import { AssetsPage } from "./Assets";
import { CareerPage } from "./Career";
import { NowPage } from "./Now";
import { PublishPage } from "./Publish";
import { SiteDataPage } from "./SiteData";
import { Works } from "./Works";

type Tab = "publish" | "works" | "now" | "about" | "career" | "assets" | "site";
const TABS: { value: Tab; label: string }[] = [
  { value: "publish", label: "发布" },
  { value: "works", label: "作品" },
  { value: "now", label: "Now" },
  { value: "about", label: "关于我" },
  { value: "career", label: "职业" },
  { value: "assets", label: "资源库" },
  { value: "site", label: "站点" },
];

function Entry(props: { label: string; onClick: () => void; dot?: boolean; busy?: string; disabled?: boolean }) {
  return (
    <button
      type="button"
      disabled={props.disabled}
      onClick={props.onClick}
      className="inline-flex items-center gap-1 rounded-control px-1.5 py-0.5 text-sm text-muted-foreground transition-colors duration-150 outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring disabled:pointer-events-none"
    >
      {props.dot && <span aria-label="在运行" className="size-1.5 rounded-full bg-[var(--status-done-fg)]" />}
      {props.busy ?? props.label}
      {!props.busy && <ExternalLink size={12} strokeWidth={1.5} />}
    </button>
  );
}

function Entries(props: { o: Overview; vercelUrl: string | null }) {
  const { o } = props;
  const qc = useQueryClient();
  const toast = useToast();
  const [starting, setStarting] = useState(false);
  const open = (url: string) => void api.openLink(url).catch((e) => toast(errorText(e), "error"));
  const running = o.server.state === "running";
  const preview = async () => {
    if (running) return open(o.server.url);
    setStarting(true);
    try {
      const s = await siteApi.serverStart();
      qc.setQueryData<Overview>(OVERVIEW_KEY, (old) => (old ? { ...old, server: s } : old));
      open(s.url);
    } catch (e) {
      toast(errorText(e), "error");
    } finally {
      setStarting(false);
      void qc.invalidateQueries({ queryKey: OVERVIEW_KEY });
    }
  };
  const stop = () =>
    void siteApi.serverStop().then(
      (s) => qc.setQueryData<Overview>(OVERVIEW_KEY, (old) => (old ? { ...old, server: s } : old)),
      (e) => toast(errorText(e), "error"),
    );
  const busy = starting || o.server.state === "starting";
  return (
    <div className="flex items-center justify-end gap-0.5">
      {o.mode === "sandbox" && <span className="mr-2 rounded-full bg-muted px-2 py-0.5 text-xs text-muted-foreground">副本</span>}
      {o.siteUrl && (
        <>
          <Entry label="正式网站" onClick={() => open(o.siteUrl!)} />
          <span className="text-muted-foreground/50">·</span>
          <Entry label="love-shuyu" onClick={() => open(`${o.siteUrl}/love-shuyu/`)} />
          <span className="text-muted-foreground/50">·</span>
        </>
      )}
      <Entry label="本地预览" dot={running} busy={busy ? "正在启动…" : undefined} disabled={busy} onClick={() => void preview()} />
      {running && o.server.ours && (
        <button type="button" onClick={stop} className="rounded-control px-1 py-0.5 text-xs text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring">
          停止
        </button>
      )}
      {o.githubUrl && (
        <>
          <span className="text-muted-foreground/50">·</span>
          <Entry label="GitHub" onClick={() => open(o.githubUrl!)} />
        </>
      )}
      {props.vercelUrl && (
        <>
          <span className="text-muted-foreground/50">·</span>
          <Entry label="Vercel" onClick={() => open(props.vercelUrl!)} />
        </>
      )}
    </div>
  );
}

function ProblemView(props: { o: Overview }) {
  const qc = useQueryClient();
  const toast = useToast();
  const navigate = useNavigate();
  const [copying, setCopying] = useState(false);
  const p = props.o.problem!;
  const copy = async () => {
    setCopying(true);
    try {
      qc.setQueryData(OVERVIEW_KEY, await siteApi.sandbox());
      void qc.invalidateQueries({ queryKey: ["site"] });
    } catch (e) {
      toast(errorText(e), "error");
    } finally {
      setCopying(false);
    }
  };
  const action =
    p.kind === "no-sandbox" ? (
      <Button variant="primary" disabled={copying} onClick={() => void copy()}>
        {copying ? "正在复制…" : "建副本"}
      </Button>
    ) : (
      <Button onClick={() => navigate("/settings")}>去设置</Button>
    );
  return <EmptyState title={p.text} hint={p.kind === "no-sandbox" ? undefined : props.o.folder} action={action} />;
}

export default function SitePage() {
  const m = moduleById("site");
  const { data: o } = useOverview();
  const ok = !!o && !o.problem;
  const { data: history } = useHistory(ok);
  const [tab, setTab] = useSubPage<Tab>("site", "ui.site_tab");
  const dirty = useRef(false);
  const [confirm, confirmDialog] = useConfirm();
  const guard = {
    get dirty() {
      return dirty.current;
    },
    confirmLeave: async () => !dirty.current || (await confirm({ title: "放弃没有保存的修改？", confirmLabel: "放弃修改", danger: true })),
    setDirty: (d: boolean) => void (dirty.current = d),
  };
  const switchTab = async (t: Tab) => {
    if (t === tab || !(await guard.confirmLeave())) return;
    dirty.current = false;
    setTab(t);
  };
  const toast = useToast();
  const qc = useQueryClient();
  const onError = (e: unknown) => toast(errorText(e), "error");
  const changed = () => void qc.invalidateQueries({ queryKey: ["site", "changes"] });
  usePageContext({ module: "site", page: TABS.find((t) => t.value === tab)!.label });
  return (
    <div className="relative flex h-full min-h-0 flex-col overflow-hidden">
      <PageHeader icon={m.icon} color={m.color} title={m.label} right={o && <Entries o={o} vercelUrl={history?.vercelUrl ?? null} />} />
      {o &&
        (o.problem ? (
          <ProblemView o={o} />
        ) : (
          <>
            <div className="shrink-0 px-8 pb-3">
              <Segmented ariaLabel="页面" value={tab} onChange={(t) => void switchTab(t)} options={TABS} />
            </div>
            {tab === "publish" ? (
              <PublishPage o={o} history={history ?? null} />
            ) : tab === "works" ? (
              <Works guard={guard} />
            ) : tab === "now" ? (
              <NowPage guard={guard} onError={onError} />
            ) : tab === "about" ? (
              <AboutPage setDirty={guard.setDirty} onError={onError} onChanged={changed} />
            ) : tab === "career" ? (
              <CareerPage setDirty={guard.setDirty} onError={onError} onChanged={changed} />
            ) : tab === "assets" ? (
              <AssetsPage onError={onError} onChanged={changed} />
            ) : (
              <SiteDataPage setDirty={guard.setDirty} onError={onError} onChanged={changed} />
            )}
          </>
        ))}
      {confirmDialog}
    </div>
  );
}
