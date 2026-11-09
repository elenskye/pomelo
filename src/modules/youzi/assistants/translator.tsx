// Translator's working parts (Rust assistants/translate.rs), shared by its page (Translate.tsx) and the 速记小窗
// (capture/CaptureTranslate.tsx): one set of settings, one 上一次, one run at a time. The page in front holds
// what is being typed and writes it after a pause; `sync` reads Rust again (the window comes to the front:
// the other one may have changed things), unless a write of its own is on its way. No router here.
import { useCallback, useEffect, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowLeftRight, Copy } from "@/shared/icons";
import { Dropdown } from "@/shared/Dropdown";
import { errorText, useToast } from "@/shared/feedback";
import { youziApi, type TranslateSettings, type TranslateView } from "../api";

export const TRANSLATE_KEY = ["youzi", "translate"];
const AUTO = "auto";
const ZH_EN = "zh-en";

export function useTranslator() {
  const qc = useQueryClient();
  const toast = useToast();
  const view = useQuery({ queryKey: TRANSLATE_KEY, queryFn: youziApi.translate, refetchInterval: (q) => (q.state.data?.running ? 300 : false), refetchIntervalInBackground: true });
  const data = view.data;
  const [input, setInput] = useState<string | null>(null);
  const [settings, setSettings] = useState<TranslateSettings | null>(null);
  // the last time comes back once; after that the page holds what is being typed
  useEffect(() => {
    if (!data) return;
    setInput((x) => x ?? data.input);
    setSettings((x) => x ?? data.settings);
  }, [data]);

  const inputTimer = useRef(0);
  const settingsTimer = useRef(0);
  const sync = useCallback(() => {
    if (inputTimer.current || settingsTimer.current) return;
    void youziApi.translate().then(
      (v) => {
        if (inputTimer.current || settingsTimer.current) return;
        qc.setQueryData(TRANSLATE_KEY, v);
        setInput(v.input);
        setSettings(v.settings);
      },
      () => {},
    );
  }, [qc]);
  useEffect(() => {
    sync();
    window.addEventListener("focus", sync);
    return () => {
      window.removeEventListener("focus", sync);
      clearTimeout(inputTimer.current);
      clearTimeout(settingsTimer.current);
      inputTimer.current = settingsTimer.current = 0;
    };
  }, [sync]);

  const typed = (v: string) => {
    setInput(v);
    clearTimeout(inputTimer.current);
    inputTimer.current = window.setTimeout(() => {
      inputTimer.current = 0;
      void youziApi.translateInput(v).catch((e) => toast(errorText(e), "error"));
    }, 500);
  };
  /** `later`: typing the paragraph (saved after a pause). */
  const change = (next: TranslateSettings, later = false) => {
    setSettings(next);
    clearTimeout(settingsTimer.current);
    settingsTimer.current = 0;
    const save = () => void youziApi.translateSettings(next).then((v) => qc.setQueryData(TRANSLATE_KEY, { ...v, input: input ?? v.input }), (e) => toast(errorText(e), "error"));
    if (later) {
      settingsTimer.current = window.setTimeout(() => {
        settingsTimer.current = 0;
        save();
      }, 500);
    } else save();
  };
  const run = () => {
    if (!settings || data?.running || !input?.trim()) return;
    clearTimeout(inputTimer.current);
    inputTimer.current = 0;
    void youziApi.translateRun(input, settings).then(
      (v) => qc.setQueryData(TRANSLATE_KEY, v),
      (e) => toast(errorText(e), "error"),
    );
  };
  const stop = () => void youziApi.translateStop().then((v) => qc.setQueryData(TRANSLATE_KEY, v), (e) => toast(errorText(e), "error"));
  return { data, input, settings, typed, change, run, stop, sync };
}

/** 源语言 ⇄ 目标语言. */
export function LanguagePickers(props: { data: TranslateView; settings: TranslateSettings; onSettings: (s: TranslateSettings) => void; menuMaxHeight?: number }) {
  const s = props.settings;
  const langs = props.data.lists.languages.map((l) => ({ value: l.code, label: l.name }));
  const canSwap = s.source !== AUTO && s.target !== ZH_EN;
  const menuMaxHeight = props.menuMaxHeight ?? 320;
  return (
    <>
      <Dropdown<string> ariaLabel="源语言" value={s.source} options={[{ value: AUTO, label: "自动检测" }, ...langs]} onChange={(v) => props.onSettings({ ...s, source: v })} menuMaxHeight={menuMaxHeight} />
      <button
        type="button"
        aria-label="对调"
        title="对调"
        disabled={!canSwap}
        onClick={() => props.onSettings({ ...s, source: s.target, target: s.source })}
        className="rounded-control p-1.5 text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-30"
      >
        <ArrowLeftRight size={14} />
      </button>
      <Dropdown<string> ariaLabel="目标语言" value={s.target} options={[{ value: ZH_EN, label: "中英互译" }, ...langs]} onChange={(v) => props.onSettings({ ...s, target: v })} menuMaxHeight={menuMaxHeight} />
    </>
  );
}

/** 译文: while it runs the status, after it the model it used and how it ended; copy. */
export function TranslateOutput(props: { data: TranslateView; className: string }) {
  const { data } = props;
  const toast = useToast();
  const [copied, setCopied] = useState(false);
  const copy = () =>
    void youziApi.copy(data.output).then(
      () => {
        setCopied(true);
        setTimeout(() => setCopied(false), 1200);
      },
      (e) => toast(errorText(e), "error"),
    );
  const stopped = data.error === "已停止";
  return (
    <section aria-label="译文" className={`flex flex-col rounded-control border bg-card ${props.className}`}>
      <div className="flex items-center gap-2 px-3 pt-2 text-xs text-muted-foreground">
        {data.running ? (
          <span className="animate-pulse">{data.status ?? "正在翻译…"}</span>
        ) : (
          <>
            {data.modelName && <span>{data.modelName}</span>}
            {data.error && <span className={stopped ? "" : "text-destructive"}>{data.error}</span>}
          </>
        )}
        {data.note && <span className="truncate">{data.note}</span>}
        <div className="flex-1" />
        <button
          type="button"
          aria-label={copied ? "已复制" : "复制"}
          title={copied ? "已复制" : "复制"}
          disabled={!data.output}
          onClick={copy}
          className="inline-flex size-7 items-center justify-center rounded-control outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-30"
        >
          {copied ? <span className="text-xs">✓</span> : <Copy size={14} strokeWidth={1.5} />}
        </button>
      </div>
      <div className="-m-1 min-h-0 flex-1 overflow-y-auto p-1">
        <p className="selectable px-3 pb-3 font-reading text-[15px] leading-relaxed whitespace-pre-wrap">{data.output}</p>
      </div>
    </section>
  );
}
