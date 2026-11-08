// 今日简报 (top of 首页; rules in Rust youzi/brief.rs): on showing, asks for today's brief once
// (written in the background, polled while writing); 重写 / 重试 write it again. Hidden when 柚子 is
// hidden or the brief is off (管理 → 定时).
import { useEffect, useRef } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { BRIEF_KEY, youziApi, useBrief, type Brief } from "@/modules/youzi/api";
import { errorText, useToast } from "@/shared/feedback";
import { InlineMd } from "@/shared/InlineMd";
import { Whisper } from "./Whisper";

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
    if (b.state === "none" || (b.state === "failed" && b.error === "上次没写完。")) void youziApi.briefWrite(false).then(put, () => {});
  }, [b, props.today]); // eslint-disable-line react-hooks/exhaustive-deps

  if (!b?.enabled) return null;
  const write = () => void youziApi.briefWrite(true).then(put, (e) => toast(errorText(e), "error"));
  const writing = b.state === "writing" || (b.state === "none" && asked.current === props.today);
  const paragraphs = b.content.split(/\n\s*\n/).map((p) => p.trim()).filter(Boolean);
  return (
    // on the desk: the Design System card with stitches down its left edge, 柚子's sticker slapped on its corner
    <section className="pm-card dk-brief" data-hand="card" data-seed={740} data-tilt={-1.2} data-tapes={1} data-stitches="">
      <Whisper className="brief-whisper" />
      <div className="dk-head">
        <button type="button" onClick={props.onTitle} className="dk-title">
          <span className="pm-card-title">今日简报</span>
        </button>
        {!writing && b.state !== "none" && (
          <button type="button" onClick={write} className="pm-card-meta" style={{ marginRight: 54 }}>
            {b.state === "failed" ? "重试" : "重写"}
          </button>
        )}
      </div>
      {writing && (
        <p className="mt-2 animate-pulse text-sm text-muted-foreground">{b.phase === "starting" ? "正在启动本地模型…" : b.content ? "正在重写…" : "正在写…"}</p>
      )}
      {b.state === "failed" && b.error && <p className="mt-2 text-sm text-destructive">{b.error}</p>}
      {paragraphs.length > 0 && (
        <div data-scroll className={`brief-body whitespace-pre-line ${writing ? "opacity-50" : ""}`}>
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
