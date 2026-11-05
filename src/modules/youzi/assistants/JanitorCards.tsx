// DevJanitor's Agent cards under a reply (Rust janitor/agent.rs): 移到废纸篓 (tick the items; a protected
// one asks again), 跑命令 (执行 runs it in the background — output shown live, 停下; one with sudo only gives
// the command to copy), 加进一键清理 (one undo step, then a rescan). 不要 closes a card. Nothing can be done
// while the reply is still being written.
import { useEffect, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { Copy } from "lucide-react";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { Button } from "@/shared/ui";
import { JANITOR_KEY, bytes, janitorApi, type JanitorCard } from "./janitorApi";

const STATE: Record<JanitorCard["state"], string> = { pending: "", running: "正在跑…", done: "做完了", failed: "没成", stopped: "停下了", cancelled: "不要了" };

function useCardActions(card: JanitorCard) {
  const qc = useQueryClient();
  const toast = useToast();
  const [busy, setBusy] = useState(false);
  const act = async (f: () => Promise<unknown>) => {
    setBusy(true);
    try {
      await f();
    } catch (e) {
      toast(errorText(e), "error");
    } finally {
      setBusy(false);
      await qc.invalidateQueries({ queryKey: ["youzi", "messages"] });
      await qc.invalidateQueries({ queryKey: JANITOR_KEY });
    }
  };
  return { busy, act, cancel: () => act(() => janitorApi.cardCancel(card.id)) };
}

function Shell(props: { title: string; card: JanitorCard; children: React.ReactNode; footer?: React.ReactNode }) {
  const c = props.card;
  return (
    <div className="mt-3 max-w-[672px] rounded-card border bg-card text-sm">
      <div className="flex flex-wrap items-baseline gap-x-2 border-b px-4 py-2">
        <span className="font-bold">{props.title}</span>
        {c.protected && <span className="rounded-full bg-accent px-1.5 text-xs text-primary">受保护 · {c.protected}</span>}
        {c.sudo && <span className="rounded-full bg-muted px-1.5 text-xs">要管理员密码</span>}
        {STATE[c.state] && <span className={`ml-auto text-xs ${c.state === "failed" ? "text-destructive" : "text-muted-foreground"}`}>{STATE[c.state]}</span>}
      </div>
      {props.children}
      {props.footer && <div className="flex items-center gap-2 border-t px-4 py-2">{props.footer}</div>}
    </div>
  );
}

function TrashCard(props: { card: JanitorCard; streaming: boolean }) {
  const c = props.card;
  const { busy, act, cancel } = useCardActions(c);
  const [confirm, dialog] = useConfirm();
  const [picked, setPicked] = useState<number[]>(() => c.items.map((_, i) => i).filter((i) => !c.items[i].protected));
  const pending = c.state === "pending";
  const size = picked.reduce((s, i) => s + (c.items[i]?.size ?? 0), 0);
  const go = async () => {
    const guarded = picked.filter((i) => c.items[i].protected);
    let ok = false;
    if (guarded.length) {
      ok = await confirm({ title: `其中 ${guarded.length} 项在受保护的地方`, message: `${guarded.map((i) => `${c.items[i].path}（${c.items[i].protected}）`).join("、")}。也移到废纸篓吗？`, confirmLabel: "也移走", danger: true });
      if (!ok && guarded.length === picked.length) return;
    }
    await act(() => janitorApi.cardTrash(c.id, ok ? picked : picked.filter((i) => !c.items[i].protected), ok));
  };
  return (
    <Shell title={c.title || "移到废纸篓"} card={c} footer={pending && <>
      <span className="text-xs text-muted-foreground tabular-nums">选了 {picked.length} 项 · {bytes(size)}</span>
      <Button className="ml-auto" disabled={busy} onClick={() => void cancel()}>不要</Button>
      <Button variant="primary" disabled={busy || props.streaming || !picked.length} onClick={() => void go()}>移到废纸篓</Button>
    </>}>
      {dialog}
      <ul className="divide-y">
        {c.items.map((it, i) => (
          <li key={it.path} className="flex items-start gap-2.5 px-4 py-2">
            {pending && <input type="checkbox" checked={picked.includes(i)} onChange={() => setPicked((p) => (p.includes(i) ? p.filter((x) => x !== i) : [...p, i]))} aria-label={it.path} className="mt-1 size-3.5 shrink-0 accent-[var(--primary)]" />}
            <div className="min-w-0 flex-1">
              <div className="flex min-w-0 flex-wrap items-baseline gap-x-2">
                <span className="min-w-0 truncate font-mono text-xs">{it.path}</span>
                {it.protected && <span className="shrink-0 rounded-full bg-accent px-1.5 text-xs text-primary">受保护 · {it.protected}</span>}
              </div>
              {it.why && <div className="text-xs text-muted-foreground">{it.why}</div>}
              {!pending && <div className={`text-xs ${it.done ? "text-[var(--status-done-fg)]" : "text-muted-foreground"}`}>{it.done ? "移到废纸篓了" : it.error ? `没移：${it.error}` : "没移"}</div>}
            </div>
            <span className="shrink-0 tabular-nums">{bytes(it.size)}</span>
          </li>
        ))}
      </ul>
    </Shell>
  );
}

function CommandCard(props: { card: JanitorCard; streaming: boolean }) {
  const toast = useToast();
  const [live, setLive] = useState(props.card);
  useEffect(() => setLive(props.card), [props.card]);
  const c = live;
  const { busy, act, cancel } = useCardActions(c);
  const [confirm, dialog] = useConfirm();
  const qc = useQueryClient();
  // a running command: its output, read again until it ends
  useEffect(() => {
    if (c.state !== "running") return;
    const t = window.setInterval(() => {
      janitorApi.card(c.id).then((x) => {
        setLive(x);
        if (x.state !== "running") {
          void qc.invalidateQueries({ queryKey: ["youzi", "messages"] });
          void qc.invalidateQueries({ queryKey: JANITOR_KEY });
        }
      }, () => {});
    }, 800);
    return () => window.clearInterval(t);
  }, [c.state, c.id, qc]);
  const run = async () => {
    let ok = false;
    if (c.protected) {
      ok = await confirm({ title: "这条命令碰到受保护的地方", message: `${c.protected}。确定要跑吗？`, confirmLabel: "照样跑", danger: true });
      if (!ok) return;
    }
    await act(async () => setLive(await janitorApi.cardRun(c.id, ok)));
  };
  const copy = () => navigator.clipboard.writeText(c.command).then(() => toast("复制了，到终端里粘贴"), () => toast("复制不了", "error"));
  const lines: [string, string][] = [["做什么", c.what], ["会删掉", c.deletes], ["能不能恢复", c.recoverable], ["大概腾出", c.frees]].filter(([, v]) => !!v) as [string, string][];
  return (
    <Shell title="跑命令" card={c} footer={c.state === "pending" ? <>
      <Button onClick={() => void copy()}><Copy size={13} /> 复制命令</Button>
      <Button className="ml-auto" disabled={busy} onClick={() => void cancel()}>不要</Button>
      {!c.sudo && <Button variant="primary" disabled={busy || props.streaming} onClick={() => void run()}>执行</Button>}
    </> : c.state === "running" ? <Button className="ml-auto" onClick={() => void act(async () => setLive(await janitorApi.cardStop(c.id)))}>停下</Button> : undefined}>
      {dialog}
      <pre className="selectable mx-4 mt-3 overflow-x-auto rounded-control bg-muted px-3 py-2 font-mono text-xs whitespace-pre-wrap">{c.command}</pre>
      <dl className="grid grid-cols-[5rem_1fr] gap-x-2 gap-y-1 px-4 py-3 text-sm">
        {lines.map(([k, v]) => <div key={k} className="contents"><dt className="text-muted-foreground">{k}</dt><dd className="min-w-0">{v}</dd></div>)}
      </dl>
      {(c.output || c.error || c.code != null) && (
        <div className="px-4 pb-3">
          {c.output && <pre className="selectable max-h-60 overflow-auto rounded-control border px-3 py-2 font-mono text-xs whitespace-pre-wrap">{c.output}</pre>}
          <div className={`mt-1 text-xs ${c.state === "failed" ? "text-destructive" : "text-muted-foreground"}`}>{c.error ?? (c.code != null ? `退出码 ${c.code}` : "")}</div>
        </div>
      )}
    </Shell>
  );
}

function RuleCard(props: { card: JanitorCard; streaming: boolean }) {
  const c = props.card;
  const { busy, act, cancel } = useCardActions(c);
  const r = c.rule;
  if (!r) return null;
  const lines: [string, string][] = [["在哪", r.pattern], ["怎么认出来", r.recognize], ["怎么重建", r.rebuild], ["默认勾上", r.checked ? "是" : "否"], ["现在有", `${c.matches} 处 · ${bytes(c.matchSize)}`]];
  return (
    <Shell title={`加进一键清理 · ${r.name}`} card={c} footer={c.state === "pending" ? <>
      <Button className="ml-auto" disabled={busy} onClick={() => void cancel()}>不要</Button>
      <Button variant="primary" disabled={busy || props.streaming} onClick={() => void act(() => janitorApi.cardRule(c.id))}>加进去</Button>
    </> : undefined}>
      <dl className="grid grid-cols-[5.5rem_1fr] gap-x-2 gap-y-1 px-4 py-3 text-sm">
        {lines.map(([k, v]) => <div key={k} className="contents"><dt className="text-muted-foreground">{k}</dt><dd className={`min-w-0 ${k === "在哪" ? "font-mono text-xs" : ""}`}>{v}</dd></div>)}
      </dl>
    </Shell>
  );
}

export function JanitorCards(props: { cards: JanitorCard[]; streaming: boolean }) {
  return (
    <div className="selectable contents">
      {props.cards.map((c) => (c.kind === "trash" ? <TrashCard key={c.id} card={c} streaming={props.streaming} /> : c.kind === "command" ? <CommandCard key={c.id} card={c} streaming={props.streaming} /> : <RuleCard key={c.id} card={c} streaming={props.streaming} />))}
    </div>
  );
}
