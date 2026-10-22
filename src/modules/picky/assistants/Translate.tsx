// 翻译 (the first 助手; rules in Rust assistants/translate.rs): two tabs. 翻译: 源语言 ⇄ 目标语言, 模型 and 思考 in
// one row, 原文 above and 译文 below (copy; while it runs the status, after it the model it used and how it
// ended). 提示词: the user's own paragraph, one keyword per group (click again to drop it) and what the
// model is told. Settings are saved as they change, the 原文 as typed; only the last time is kept. A run
// goes on in Rust when the page is left: the page polls while it runs. ⌘↩ in 原文 translates.
import { useEffect, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowLeftRight, Copy, Lightbulb } from "lucide-react";
import { Dropdown } from "@/shared/Dropdown";
import { errorText, useToast } from "@/shared/feedback";
import { Button, Segmented, TextArea } from "@/shared/ui";
import { pickyApi, usePickyStatus, type TranslateSettings, type TranslateView } from "../api";
import { useModels } from "../Schedules";
import { thinkOf } from "../Thinking";

export const TRANSLATE_KEY = ["picky", "translate"];
const AUTO = "auto";
const ZH_EN = "zh-en";

export function TranslatePage() {
  const qc = useQueryClient();
  const toast = useToast();
  const [tab, setTab] = useState<"translate" | "prompt">("translate");
  const view = useQuery({ queryKey: TRANSLATE_KEY, queryFn: pickyApi.translate, refetchInterval: (q) => (q.state.data?.running ? 300 : false) });
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
  useEffect(
    () => () => {
      clearTimeout(inputTimer.current);
      clearTimeout(settingsTimer.current);
    },
    [],
  );
  const typed = (v: string) => {
    setInput(v);
    clearTimeout(inputTimer.current);
    inputTimer.current = window.setTimeout(() => void pickyApi.translateInput(v).catch((e) => toast(errorText(e), "error")), 500);
  };
  /** `later`: typing the paragraph (saved after a pause). */
  const change = (next: TranslateSettings, later = false) => {
    setSettings(next);
    clearTimeout(settingsTimer.current);
    const save = () => void pickyApi.translateSettings(next).then((v) => qc.setQueryData(TRANSLATE_KEY, { ...v, input: input ?? v.input }), (e) => toast(errorText(e), "error"));
    if (later) settingsTimer.current = window.setTimeout(save, 500);
    else save();
  };

  if (!data || input == null || !settings) return <div className="flex-1" />;
  const running = data.running;
  const run = () => {
    if (running || !input.trim()) return;
    clearTimeout(inputTimer.current);
    void pickyApi.translateRun(input, settings).then(
      (v) => qc.setQueryData(TRANSLATE_KEY, v),
      (e) => toast(errorText(e), "error"),
    );
  };
  const stop = () => void pickyApi.translateStop().then((v) => qc.setQueryData(TRANSLATE_KEY, v), (e) => toast(errorText(e), "error"));

  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col px-8 pb-1">
      <div className="mx-auto flex min-h-0 w-full max-w-4xl flex-1 flex-col gap-3">
        <div>
          <Segmented
            ariaLabel="Translator"
            value={tab}
            onChange={setTab}
            options={[
              { value: "translate", label: "Translator" },
              { value: "prompt", label: "提示词" },
            ]}
          />
        </div>
        {tab === "translate" ? (
          <TranslateTab data={data} input={input} settings={settings} onInput={typed} onSettings={change} onRun={run} onStop={stop} />
        ) : (
          <PromptTab data={data} settings={settings} onSettings={change} />
        )}
      </div>
    </div>
  );
}

function TranslateTab(props: {
  data: TranslateView;
  input: string;
  settings: TranslateSettings;
  onInput: (v: string) => void;
  onSettings: (s: TranslateSettings) => void;
  onRun: () => void;
  onStop: () => void;
}) {
  const { data, settings: s } = props;
  const toast = useToast();
  const status = usePickyStatus().data;
  const { models, keep } = useModels(s.model, s.thinking);
  const [copied, setCopied] = useState(false);
  const langs = data.lists.languages.map((l) => ({ value: l.code, label: l.name }));
  const canSwap = s.source !== AUTO && s.target !== ZH_EN;
  const ability = thinkOf(status, s.model ?? status?.selection ?? null);
  const fixed = ability !== "switch";
  const thinking = ability === "always" || (ability === "switch" && s.thinking);
  const copy = () =>
    void pickyApi.copy(data.output).then(
      () => {
        setCopied(true);
        setTimeout(() => setCopied(false), 1200);
      },
      (e) => toast(errorText(e), "error"),
    );
  const stopped = data.error === "已停止";
  return (
    <>
      <div className="flex flex-wrap items-center gap-2">
        <Dropdown<string> ariaLabel="源语言" value={s.source} options={[{ value: AUTO, label: "自动检测" }, ...langs]} onChange={(v) => props.onSettings({ ...s, source: v })} menuMaxHeight={320} />
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
        <Dropdown<string> ariaLabel="目标语言" value={s.target} options={[{ value: ZH_EN, label: "中英互译" }, ...langs]} onChange={(v) => props.onSettings({ ...s, target: v })} menuMaxHeight={320} />
        <span className="mx-1 h-4 border-l" />
        <Dropdown<string | null> ariaLabel="模型" value={s.model} options={models} foldKey="models" onChange={(v) => props.onSettings({ ...s, model: v })} menuMaxHeight={360} />
        <button
          type="button"
          aria-pressed={thinking}
          aria-disabled={fixed}
          title={ability === "always" ? "这个模型只能思考" : ability === "never" ? "这个模型不能思考" : undefined}
          onClick={() => !fixed && props.onSettings({ ...s, thinking: !s.thinking, model: keep(s.model, !s.thinking) })}
          className={`flex items-center gap-1 rounded-control px-2 py-1 text-sm text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring ${thinking ? "bg-accent text-accent-foreground" : ""} ${fixed ? "cursor-default opacity-50" : ""}`}
        >
          <Lightbulb size={14} strokeWidth={1.5} /> 思考
        </button>
        <div className="flex-1" />
        {data.running ? (
          <Button onClick={props.onStop}>停止</Button>
        ) : (
          <Button variant="primary" disabled={!props.input.trim()} onClick={props.onRun}>
            翻译
          </Button>
        )}
      </div>
      <TextArea
        aria-label="原文"
        value={props.input}
        readOnly={data.running}
        onChange={(e) => props.onInput(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && (e.metaKey || e.ctrlKey) && !e.nativeEvent.isComposing) {
            e.preventDefault();
            props.onRun();
          }
        }}
        className="min-h-24 flex-1 font-reading text-[15px]"
      />
      <section aria-label="译文" className="flex min-h-24 flex-1 flex-col rounded-control border bg-card">
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
    </>
  );
}

function PromptTab(props: { data: TranslateView; settings: TranslateSettings; onSettings: (s: TranslateSettings, later?: boolean) => void }) {
  const s = props.settings;
  const preview = useQuery({ queryKey: ["picky", "translate-preview", JSON.stringify({ ...s, model: null })], queryFn: () => pickyApi.translatePreview(s) });
  const pick = (group: string, choice: string) => {
    const keywords = { ...s.keywords };
    if (keywords[group] === choice) delete keywords[group];
    else keywords[group] = choice;
    props.onSettings({ ...s, keywords });
  };
  return (
    <div className="-m-1 flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto p-1 pb-4 text-sm">
      <div className="flex flex-col gap-1.5">
        <span>自己写的提示词</span>
        <TextArea aria-label="自己写的提示词" rows={5} maxLength={2000} value={s.prompt} onChange={(e) => props.onSettings({ ...s, prompt: e.target.value }, true)} className="font-reading" />
      </div>
      <div className="flex flex-col gap-2.5">
        {props.data.lists.groups.map((g) => (
          <div key={g.key} className="flex items-center gap-3">
            <span className="w-20 shrink-0 text-muted-foreground">{g.name}</span>
            <div role="radiogroup" aria-label={g.name} className="flex flex-wrap gap-1.5">
              {g.choices.map((c) => {
                const on = s.keywords[g.key] === c.key;
                return (
                  <button
                    key={c.key}
                    type="button"
                    role="radio"
                    aria-checked={on}
                    onClick={() => pick(g.key, c.key)}
                    className={`rounded-control border px-2.5 py-1 outline-none focus-visible:ring-2 focus-visible:ring-ring ${on ? "border-primary bg-accent text-accent-foreground" : "hover:bg-muted"}`}
                  >
                    {c.label}
                  </button>
                );
              })}
            </div>
          </div>
        ))}
      </div>
      {preview.data && (
        <div className="flex flex-col gap-1.5 border-t pt-4">
          <span className="text-muted-foreground">发给模型的说明</span>
          <p className="selectable font-reading leading-relaxed whitespace-pre-wrap text-muted-foreground">{preview.data.replace(/^【小拾翻译】/, "")}</p>
        </div>
      )}
    </div>
  );
}
