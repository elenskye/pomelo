// A sticky note that slaps on and peels itself off after a while, optionally with a handful of tiny stickers flying
// out (Design System motion part done-cheer): 「今天的事都做完啦！」 when the day's last task is ticked, 「别戳啦，晕了…」
// when 柚子 is poked too often (小彩蛋). `cheer(text, x, y, confetti)` from anywhere; <CheerLayer/> once in the frame.
import { useSyncExternalStore } from "react";
import { reducedMotion } from "./hand";

type Note = { id: number; text: string; x: number; y: number; bits: string | null; leaving: boolean };
let notes: Note[] = [];
let seq = 1;
const listeners = new Set<() => void>();
const emit = () => listeners.forEach((l) => l());

const COLORS = ["yuzu", "cat-pink", "cat-sky", "cat-mint", "cat-grape", "cat-orange"];
const SHAPES = ["pm-cf-dot", "pm-cf-sq", "pm-cf-star"];
/** A handful of tiny stickers: where each flies, how far it turns, how late it starts. */
function confetti() {
  let out = "";
  for (let k = 0; k < 20; k++) {
    const a = -Math.PI / 2 + (Math.random() - 0.5) * Math.PI * 1.4, d = 80 + Math.random() * 140, sz = 8 + Math.round(Math.random() * 6);
    out += `<i class="${SHAPES[k % 3]}" style="width:${sz}px;height:${sz}px;background:var(--color-${COLORS[k % 6]});--dx:${Math.round(Math.cos(a) * d)}px;--dy:${Math.round(Math.sin(a) * d)}px;--rr:${Math.round(Math.random() * 540 - 270)}deg;--pm-d:${(Math.random() * 0.1).toFixed(2)}s"></i>`;
  }
  return out;
}

/** A note centred on (x, y) in the window; `withConfetti` throws the tiny stickers from there too. */
export function cheer(text: string, x: number, y: number, withConfetti = false) {
  const id = seq++;
  notes = [...notes, { id, text, x, y, bits: withConfetti && !reducedMotion() ? confetti() : null, leaving: false }];
  emit();
  setTimeout(() => {
    notes = notes.map((n) => (n.id === id ? { ...n, leaving: true } : n));
    emit();
  }, 2400);
  setTimeout(() => {
    notes = notes.filter((n) => n.id !== id);
    emit();
  }, 2800);
}

/** Cheer over an element (the list that was just finished). */
export const cheerOver = (el: Element | null, text: string, withConfetti = true) => {
  const r = el?.getBoundingClientRect();
  cheer(text, r ? r.left + r.width / 2 : window.innerWidth / 2, r ? r.top + Math.min(r.height / 2, 160) : window.innerHeight / 2, withConfetti);
};

export function CheerLayer() {
  const list = useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => notes,
  );
  return (
    <>
      {list.map((n) => (
        <div key={n.id} className={`cheer pm-cheer ${n.leaving ? "is-leaving" : "is-in"}`} style={{ left: n.x, top: n.y }} role="status">
          <div className="pm-cheer-note">
            <div className="pm-note cheer-note" data-hand="note" data-seed="1455" data-tilt="-3" data-tapes="1">
              {n.text}
            </div>
          </div>
          {n.bits && <div className="pm-confetti" dangerouslySetInnerHTML={{ __html: n.bits }} />}
        </div>
      ))}
    </>
  );
}
