import { useEffect, useState } from "react";
import { X } from "lucide-react";
import { Dropdown } from "./Dropdown";

export const currentMonth = () => {
  const d = new Date();
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}`;
};

/** 'YYYY-MM' | null via two native selects (WebKit has no <input type="month">). Emits null until both parts are set. */
export function MonthInput(props: { value: string | null; onChange: (v: string | null) => void }) {
  const [year, setYear] = useState(props.value?.slice(0, 4) ?? "");
  const [month, setMonth] = useState(props.value?.slice(5, 7) ?? "");

  useEffect(() => {
    const local = year && month ? `${year}-${month}` : null;
    if (local !== props.value) {
      setYear(props.value?.slice(0, 4) ?? "");
      setMonth(props.value?.slice(5, 7) ?? "");
    }
  }, [props.value]);

  const emit = (y: string, m: string) => {
    setYear(y);
    setMonth(m);
    props.onChange(y && m ? `${y}-${m}` : null);
  };

  const thisYear = new Date().getFullYear();
  const years = Array.from({ length: thisYear - 1950 + 2 }, (_, i) => String(thisYear + 1 - i));
  const months = Array.from({ length: 12 }, (_, i) => String(i + 1).padStart(2, "0"));

  return (
    <div className="flex items-center gap-2">
      <Dropdown
        ariaLabel="年"
        value={year}
        onChange={(y) => emit(y, month || (y ? "01" : ""))}
        placeholder="年"
        menuMaxHeight={260}
        options={years.map((y) => ({ value: y, label: `${y} 年` }))}
      />
      <Dropdown
        ariaLabel="月"
        value={month}
        onChange={(m) => emit(year || String(new Date().getFullYear()), m)}
        placeholder="月"
        menuMaxHeight={260}
        options={months.map((m) => ({ value: m, label: `${Number(m)} 月` }))}
      />
      {(year || month) && (
        <button
          type="button"
          aria-label="清除完成时间"
          onClick={() => emit("", "")}
          className="rounded-control p-1.5 text-muted-foreground hover:bg-muted"
        >
          <X size={14} />
        </button>
      )}
    </div>
  );
}
