// 今日简报 (top of 首页; rules in Rust picky/brief.rs): on showing, asks for today's brief once
// (written in the background, polled while writing); 重写 / 重试 write it again. Hidden when 小拾 is
// hidden or the brief is off (管理 → 定时).
import { useEffect, useRef } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { Bird } from "lucide-react";
import { BRIEF_KEY, pickyApi, useBrief, type Brief } from "@/modules/picky/api";
import { errorText, useToast } from "@/shared/feedback";
import { InlineMd } from "@/shared/InlineMd";

export function BriefCard(props: { today: string; onTitle: () => void }) {
  const qc = useQueryClient();
  const toast = useToast();
  const { data: b } = useBrief();
  const asked = useRef<string | null>(null);
  const put = (v: Brief) => qc.setQueryData(BRIEF_KEY, v);
  // the first showing each day writes it (Rust skips it when there's one)
  useEffect(() => {
    if (!b?.enabled || asked.current === props.today) return;
    if (b.day !== props.today) return void qc.invalidateQueries({ queryKey: BRIEF_KEY });
    asked.current = props.today;
    if (b.state === "none" || (b.state === "failed" && b.error === "上次没写完。")) void pickyApi.briefWrite(false).then(put, () => {});
  }, [b, props.today]); // eslint-disable-line react-hooks/exhaustive-deps

  if (!b?.enabled) return null;
  const write = () => void pickyApi.briefWrite(true).then(put, (e) => toast(errorText(e), "error"));
  const writing = b.state === "writing" || (b.state === "none" && asked.current === props.today);
  const paragraphs = b.content.split(/\n\s*\n/).map((p) => p.trim()).filter(Boolean);
  return (
    <section className="rounded-card border bg-card px-4 py-3">
      <div className="flex items-center gap-1.5">
        <button type="button" onClick={props.onTitle} className="flex items-center gap-1.5 rounded-control text-sm font-bold outline-none focus-visible:ring-2 focus-visible:ring-ring">
          <Bird size={15} strokeWidth={1.75} style={{ color: "var(--module-picky)" }} />
          今日简报
        </button>
        <div className="flex-1" />
        {!writing && b.state !== "none" && (
          <button type="button" onClick={write} className="rounded-control px-1.5 py-0.5 text-xs text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring">
            {b.state === "failed" ? "重试" : "重写"}
          </button>
        )}
      </div>
      {writing && (
        <p className="mt-2 animate-pulse text-sm text-muted-foreground">{b.phase === "starting" ? "正在启动本地模型…" : b.content ? "正在重写…" : "正在写…"}</p>
      )}
      {b.state === "failed" && b.error && <p className="mt-2 text-sm text-destructive">{b.error}</p>}
      {paragraphs.length > 0 && (
        <div className={`mt-2 space-y-1.5 text-sm leading-relaxed whitespace-pre-line ${writing ? "opacity-50" : ""}`}>
          {paragraphs.map((p, i) => (
            <p key={i}>
              <InlineMd text={p.replace(/^#+\s*/gm, "")} />
            </p>
          ))}
        </div>
      )}
      {b.state === "done" && b.leftOut.length > 0 && <p className="mt-2 text-xs text-muted-foreground">没看：{b.leftOut.join("、")}（不发给云端）</p>}
    </section>
  );
}
