// 翻译 (the first 助手; rules in Rust assistants/translate.rs): two tabs. 翻译: 源语言 ⇄ 目标语言, 模型 and 思考 in
// one row, 原文 above and 译文 below (copy; while it runs the status, after it the model it used and how it
// ended). 提示词: the user's own paragraph, one keyword per group (click again to drop it) and what the
// model is told. Settings are saved as they change, the 原文 as typed; only the last time is kept. A run
// goes on in Rust when the page is left: the page polls while it runs. ⌘↩ in 原文 translates. The working
// parts are in translator.tsx (the 速记小窗 translates too).
import { useQuery } from "@tanstack/react-query";
import { Dropdown } from "@/shared/Dropdown";
import { Button, Segmented, TextArea } from "@/shared/ui";
import { youziApi, type TranslateSettings, type TranslateView } from "../api";
import { useModels } from "../Schedules";
import { ThinkingButton, fromLevel } from "../Thinking";
import { useSubPage } from "@/app/subpage";
import { LanguagePickers, TranslateOutput, useTranslator } from "./translator";

export function TranslatePage() {
  const [tab, setTab] = useSubPage<"translate" | "prompt">("translate");
  const { data, input, settings, typed, change, run, stop } = useTranslator();
  if (!data || input == null || !settings) return <div className="flex-1" />;
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
  const { models, keep } = useModels(s.model, s.thinking);
  return (
    <>
      <div className="flex flex-wrap items-center gap-2">
        <LanguagePickers data={data} settings={s} onSettings={props.onSettings} />
        <span className="mx-1 h-4 border-l" />
        <Dropdown<string | null> ariaLabel="模型" value={s.model} options={models} foldKey="models" onChange={(v) => props.onSettings({ ...s, model: v })} menuMaxHeight={360} />
        <ThinkingButton
          model={s.model}
          on={s.thinking}
          effort={s.effort ?? null}
          onLevel={(l) => {
            const v = fromLevel(l);
            props.onSettings({ ...s, ...v, model: keep(s.model, v.thinking) });
          }}
          className="flex items-center gap-1 rounded-control px-2 py-1 text-sm text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
        />
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
      <TranslateOutput data={data} className="min-h-24 flex-1" />
    </>
  );
}

function PromptTab(props: { data: TranslateView; settings: TranslateSettings; onSettings: (s: TranslateSettings, later?: boolean) => void }) {
  const s = props.settings;
  const preview = useQuery({ queryKey: ["youzi", "translate-preview", JSON.stringify({ ...s, model: null })], queryFn: () => youziApi.translatePreview(s) });
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
          <p className="selectable font-reading leading-relaxed whitespace-pre-wrap text-muted-foreground">{preview.data.replace(/^【柚子翻译】/, "")}</p>
        </div>
      )}
    </div>
  );
}
