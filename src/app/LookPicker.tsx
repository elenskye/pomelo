// 图标和颜色: pick a module's sticker (its own, another module's, or a general one) and one of the 5 category
// colors. In the 模块坞 (right-click or 「⋯」 →「图标和颜色…」) and when making a 五花八门 / 循序渐进.
// Saved in settings (looks.ts); a made module also keeps the icon name of its sticker when there is one, so its
// small icon in 页签 and search matches. What another module wears (the same sticker in the same color) is greyed
// out and can't be picked: the 柚子瓣 shows stickers only.
import { useState, type FormEvent } from "react";
import { useCustomMutations } from "@/modules/custom/api";
import { iconForMark, MADE_MARKS } from "@/modules/custom/icons";
import { Mark, STICKERS, stickerById, TINT_NAMES, TINTS, tintColor, type Tint } from "@/shared/marks";
import { Modal } from "@/shared/Modal";
import { Button } from "@/shared/ui";
import { freeTint, lookFor, lookKey, ownMark, ownTint, useLooks } from "./looks";
import { ModuleIcon } from "./ModuleIcon";
import { takenBy, useModuleLooks } from "./moduleLooks";
import type { ModuleDef } from "./modules";

/** The colors and the stickers; `mark` / `tint` are what the module shows; `taken` is what the others wear (lookKey → name). */
export function LookPicker(props: { mark: string; tint: Tint | null; taken: Map<string, string>; onMark: (id: string) => void; onTint: (t: Tint) => void }) {
  const group = (own: boolean) => STICKERS.filter((s) => s.own === own);
  const grid = (label: string, list: typeof STICKERS) =>
    list.length > 0 && (
      <div className="space-y-1">
        <p className="text-xs text-muted-foreground">{label}</p>
        <div role="radiogroup" aria-label={`${label}贴纸`} className="grid grid-cols-[repeat(auto-fill,40px)] gap-0.5">
          {list.map((s) => {
            const on = s.id === props.mark;
            const who = on ? undefined : props.taken.get(lookKey(s.id, props.tint));
            return (
              <button
                key={s.id}
                type="button"
                role="radio"
                aria-checked={on}
                aria-label={who ? `${s.name}（${who}在用）` : s.name}
                title={who ? `${who}在用` : s.name}
                disabled={!!who}
                onClick={() => props.onMark(s.id)}
                className={`grid size-10 place-items-center rounded-[10px] outline-none transition-[background-color,transform,opacity] duration-150 focus-visible:ring-2 focus-visible:ring-ring active:scale-90 disabled:pointer-events-none disabled:opacity-25 ${on ? "bg-accent ring-1 ring-primary/40" : "hover:bg-muted"}`}
              >
                <Mark svg={s.svg} size={32} tint={props.tint} />
              </button>
            );
          })}
        </div>
      </div>
    );
  return (
    <div className="space-y-3">
      <div role="radiogroup" aria-label="颜色" className="flex gap-3">
        {TINTS.map((t) => {
          const on = t === props.tint;
          const who = on ? undefined : props.taken.get(lookKey(props.mark, t));
          return (
            <button
              key={t}
              type="button"
              role="radio"
              aria-checked={on}
              aria-label={who ? `${TINT_NAMES[t]}（${who}在用）` : TINT_NAMES[t]}
              title={who ? `${who}在用这张贴纸的${TINT_NAMES[t]}` : undefined}
              disabled={!!who}
              onClick={() => props.onTint(t)}
              className="group/sw flex flex-col items-center gap-1 rounded-control px-1 py-0.5 text-xs text-muted-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-30"
            >
              <span
                className="size-[26px] rounded-full transition-transform duration-150 group-hover/sw:scale-110 group-active/sw:scale-90"
                style={{ background: tintColor(t), boxShadow: on ? "0 0 0 2px var(--popover), 0 0 0 4px var(--foreground)" : "0 0 0 2px var(--popover)" }}
              />
              <span className={on ? "text-foreground" : ""}>{TINT_NAMES[t]}</span>
            </button>
          );
        })}
      </div>
      <div className="max-h-[300px] space-y-2.5 overflow-y-auto pr-1">
        {grid("模块", group(true))}
        {grid("更多", group(false))}
      </div>
    </div>
  );
}

const madeId = (key: string) => {
  const m = /^(?:list|progress)\/(\d+)$/.exec(key);
  return m ? Number(m[1]) : null;
};

/** 「图标和颜色…」 for one module (not 柚子: it is 柚子). */
export function LookDialog(props: { def: ModuleDef; onClose: () => void }) {
  const def = props.def;
  const looks = useLooks();
  const { defs, eff } = useModuleLooks();
  const custom = useCustomMutations();
  const taken = takenBy(defs, eff, def.id);
  const now = eff.get(def.id);
  const [mark, setMark] = useState(now?.mark ?? ownMark(def));
  const [tint, setTint] = useState<Tint | null>(now ? now.tint : ownTint(def));
  const own = { mark: ownMark(def), tint: ownTint(def) };
  // 「恢复默认」: its own sticker, in its own color unless another module wears that
  const back = { mark: own.mark, tint: taken.has(lookKey(own.mark, own.tint)) ? freeTint(own.mark, own.tint, taken) : own.tint };
  const preview = lookFor(own, mark, tint);
  const [error, setError] = useState<string | null>(null);

  const submit = (e: FormEvent) => {
    e.preventDefault();
    const who = taken.get(lookKey(mark, tint));
    if (who) return setError(`和「${who}」一模一样了，换一张贴纸或一种颜色`);
    const id = madeId(def.id);
    const icon = id !== null ? iconForMark(mark) : null;
    if (id !== null && icon && MADE_MARKS[icon] !== own.mark) {
      // a made module whose new sticker has an icon: that icon becomes its own, the look keeps only the color
      custom.update.mutate({ id, patch: { icon } });
      looks.set(def.id, lookFor({ mark, tint: stickerById(mark)?.tint ?? null }, mark, tint));
    } else looks.set(def.id, preview);
    props.onClose();
  };

  return (
    <Modal
      title="图标和颜色"
      wide
      onClose={props.onClose}
      onSubmit={submit}
      error={error}
      submitLabel="好了"
      footerLeft={
        <Button
          onClick={() => {
            setMark(back.mark);
            setTint(back.tint);
          }}
          disabled={mark === back.mark && tint === back.tint}
        >
          恢复默认
        </Button>
      }
    >
      <div className="flex items-center gap-3">
        <ModuleIcon def={def} size={48} look={preview} />
        <span className="text-[17px] font-bold">{def.label}</span>
      </div>
      <LookPicker mark={mark} tint={tint} taken={taken} onMark={setMark} onTint={setTint} />
    </Modal>
  );
}
