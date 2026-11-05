// 速记小窗 · 翻译: 柚子's Translator in the window (assistants/translator.tsx): the same settings, the same 上一次
// and the same single run as its page; a run goes on when the window hides. ↩ or ⌘↩ translates, ⇧↩ a new line.
// The window is taller once there's a result. Loaded only when the tab is.
import { useEffect, type Ref } from "react";
import { createPortal } from "react-dom";
import { listen } from "@tauri-apps/api/event";
import { Languages, Square } from "lucide-react";
import { Dropdown } from "@/shared/Dropdown";
import { Button, TextArea } from "@/shared/ui";
import { toolClass } from "@/modules/youzi/Composer";
import { useModels } from "@/modules/youzi/Schedules";
import { ThinkingButton, fromLevel } from "@/modules/youzi/Thinking";
import { LanguagePickers, TranslateOutput, useTranslator } from "@/modules/youzi/assistants/translator";
import { captureApi } from "./api";

export default function CaptureTranslate(props: { inputRef: Ref<HTMLTextAreaElement>; slot: HTMLElement | null }) {
  const { data, input, settings: s, typed, change, run, stop, sync } = useTranslator();
  const { models, keep } = useModels(s?.model ?? null, s?.thinking ?? false);
  const result = !!data && (data.running || !!data.output || !!data.error);

  useEffect(() => void captureApi.fit(result).catch(() => {}), [result]);
  useEffect(() => () => void captureApi.fit(false).catch(() => {}), []);
  useEffect(() => {
    const off = listen("capture-shown", sync);
    return () => void off.then((f) => f());
  }, [sync]);

  if (!data || input == null || !s) return <div className="flex-1" />;
  return (
    <>
      {props.slot &&
        createPortal(<Dropdown<string | null> ariaLabel="模型" value={s.model} options={models} foldKey="models" onChange={(v) => change({ ...s, model: v })} menuMaxHeight={240} />, props.slot)}
      <div className="flex min-h-0 flex-1 flex-col gap-2">
        <TextArea
          ref={props.inputRef}
          aria-label="原文"
          value={input}
          readOnly={data.running}
          onChange={(e) => typed(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing && e.keyCode !== 229) {
              e.preventDefault();
              run();
            }
          }}
          placeholder="要翻译的文字……"
          className="min-h-0 flex-1 font-reading text-[15px]"
          autoFocus
        />
        {result && <TranslateOutput data={data} className="min-h-0 flex-1" />}
      </div>
      <footer className="flex shrink-0 items-center gap-1.5">
        <LanguagePickers data={data} settings={s} onSettings={change} menuMaxHeight={200} />
        <ThinkingButton
          model={s.model}
          on={s.thinking}
          effort={s.effort ?? null}
          onLevel={(l) => {
            const v = fromLevel(l);
            change({ ...s, ...v, model: keep(s.model, v.thinking) });
          }}
          className={toolClass}
        />
        <span className="flex-1" />
        {data.running ? (
          <Button onClick={stop}>
            <Square size={12} className="fill-current" /> 停止
          </Button>
        ) : (
          <Button variant="primary" disabled={!input.trim()} onClick={run}>
            <Languages size={14} /> 翻译
          </Button>
        )}
      </footer>
    </>
  );
}
