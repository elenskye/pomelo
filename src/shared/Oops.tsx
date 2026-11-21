// A part of the window that failed to draw shows what went wrong in its own place, instead of React taking the whole
// window down with it (a white window until restart). Each page on the desk, the desk itself and every layer over it
// (贴纸簿、柚子瓣、找东西、问柚子的小窗) has its own; the page's resets when you go to another page.
import { Component, useSyncExternalStore, type ErrorInfo, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "./ui";

type Props = {
  children: ReactNode;
  /** Where it is (shown with the error, so it can be told apart). */
  where: string;
  /** A different key (the page's path) clears the error and draws it again. */
  reset?: unknown;
  /** `page` sits in the paper, `layer` is a small note in the corner (the layer itself is gone), `window` is everything. */
  look?: "page" | "layer" | "window";
  /** A layer that failed: put it away (its owner closes it). */
  onClose?: () => void;
};
type State = { error: Error | null; stack: string; reset: unknown; copied: boolean };

const text = (s: State, where: string) => `${where}\n${s.error?.name ?? "Error"}: ${s.error?.message ?? ""}\n${s.error?.stack ?? ""}\n${s.stack}`.trim();

// dev only (docs/testing.md「出错兜底」): window.__pomeloCrash("首页" | a page's path | "贴纸簿"…) makes that part throw once
let crashAt: string | null = null;
const crashSubs = new Set<() => void>();
if (import.meta.env.DEV && typeof window !== "undefined")
  (window as unknown as { __pomeloCrash: (where: string) => void }).__pomeloCrash = (where) => {
    crashAt = where;
    crashSubs.forEach((f) => f());
  };
function Probe(props: { where: string }) {
  const at = useSyncExternalStore((f) => (crashSubs.add(f), () => void crashSubs.delete(f)), () => crashAt);
  if (at === props.where) throw new Error(`试出来的错（${props.where}）`);
  return null;
}

export class Oops extends Component<Props, State> {
  state: State = { error: null, stack: "", reset: this.props.reset, copied: false };

  static getDerivedStateFromError(error: unknown): Partial<State> {
    return { error: error instanceof Error ? error : new Error(String(error)) };
  }

  static getDerivedStateFromProps(props: Props, state: State): Partial<State> | null {
    return props.reset !== state.reset ? { error: null, stack: "", reset: props.reset, copied: false } : null;
  }

  componentDidCatch(error: unknown, info: ErrorInfo) {
    if (import.meta.env.DEV && crashAt === this.props.where) crashAt = null;
    this.setState({ stack: info.componentStack ?? "" });
    console.error(`[${this.props.where}]`, error, info.componentStack);
  }

  private copy = () => {
    const t = text(this.state, this.props.where);
    invoke("copy_text", { text: t })
      .catch(() => navigator.clipboard?.writeText(t))
      .then(() => this.setState({ copied: true }))
      .catch(() => {});
  };

  render() {
    const { error } = this.state;
    if (!error)
      return (
        <>
          {import.meta.env.DEV && <Probe where={this.props.where} />}
          {this.props.children}
        </>
      );
    const look = this.props.look ?? "page";
    const again = () => (look === "window" ? location.reload() : this.setState({ error: null, stack: "", copied: false }));
    const body = (
      <>
        <p className="text-base font-bold">{look === "layer" ? "这一块出错了" : look === "window" ? "柚儿园出错了" : "这一页出错了"}</p>
        <p className="selectable max-h-40 overflow-auto text-xs leading-relaxed break-all text-muted-foreground">
          {this.props.where} · {error.message || error.name}
        </p>
        <div className="flex gap-2">
          {look === "layer" ? (
            this.props.onClose && <Button onClick={() => { this.setState({ error: null, stack: "", copied: false }); this.props.onClose?.(); }}>收起</Button>
          ) : (
            <Button onClick={again}>{look === "window" ? "重新载入" : "再试一次"}</Button>
          )}
          <Button onClick={this.copy}>{this.state.copied ? "已复制" : "复制错误"}</Button>
        </div>
      </>
    );
    if (look === "layer")
      return (
        <div role="alert" className="pm-card fixed right-6 bottom-6 z-50 flex max-w-sm flex-col gap-2" data-hand="card" data-seed="1438">
          {body}
        </div>
      );
    return (
      <div role="alert" className={`grid place-items-center p-10 ${look === "window" ? "h-full bg-sidebar" : "min-h-[60%]"}`}>
        <div className="pm-card flex max-w-xl flex-col gap-3" data-hand="card" data-seed="1439">
          {body}
        </div>
      </div>
    );
  }
}
