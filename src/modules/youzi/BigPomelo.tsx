// 柚子 on the desk beside its notepad (Phase 2 final design): it thinks while a reply is being written in the open
// conversation, is happy for a moment when it's done, looks up wide-eyed while it waits for an answer (问我). Poked, it
// changes its face — and 5 pokes in 2.5 s make it dizzy (小彩蛋, with a note stuck beside it). The pomelo in the top bar
// follows each poke (the frame's `pomelo:face` cue).
import { useEffect, useRef, useState } from "react";
import { PomeloFace, type Face } from "@/app/Pomelo";
import { cheer } from "@/shared/cheer";
import { reducedMotion } from "@/shared/hand";

const POKED: Face[] = ["wow", "success", "wink", "surprised"];

export function BigPomelo(props: { writing: boolean; asking: boolean; size: number }) {
  const [poked, setPoked] = useState<Face | null>(null);
  const [dizzy, setDizzy] = useState(false);
  const [done, setDone] = useState(false);
  const pokes = useRef<number[]>([]);
  const timer = useRef(0);
  const was = useRef(props.writing);
  useEffect(() => {
    const before = was.current;
    was.current = props.writing;
    if (!before || props.writing) return setDone(false);
    setDone(true);
    const t = setTimeout(() => setDone(false), 2800);
    return () => clearTimeout(t);
  }, [props.writing]);
  useEffect(() => () => clearTimeout(timer.current), []);
  const show = (face: Face, ms: number) => {
    setPoked(face);
    window.dispatchEvent(new CustomEvent("pomelo:face", { detail: { face, ms } }));
    clearTimeout(timer.current);
    timer.current = window.setTimeout(() => {
      setPoked(null);
      setDizzy(false);
    }, ms);
  };
  const face: Face = poked ?? (props.asking ? "wow" : props.writing ? "thinking" : done ? "success" : "idle");
  const w = Math.round(props.size * 1.1);
  return (
    <div className="yz-pomelo">
      <button
        type="button"
        aria-label="戳一下柚子"
        className="pm-press"
        onClick={(e) => {
          if (dizzy) return;
          const now = Date.now();
          pokes.current = pokes.current.filter((t) => now - t < 2500).concat(now);
          if (pokes.current.length >= 5) {
            pokes.current = [];
            setDizzy(!reducedMotion());
            show("error", 2200);
            const r = e.currentTarget.getBoundingClientRect();
            cheer("别戳啦，晕了…", r.right + 90, r.top + 40);
            return;
          }
          show(POKED[pokes.current.length % 4], 1000);
        }}
      >
        <PomeloFace face={face} size={props.size} dizzy={dizzy} />
      </button>
      <svg width={w} height={18} aria-hidden>
        <ellipse cx={w / 2} cy={9} rx={w * 0.44} ry={7} />
      </svg>
      <span className="yz-poke">戳戳我</span>
    </div>
  );
}
