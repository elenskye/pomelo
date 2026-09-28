import { useEffect, useState } from "react";
import { X } from "lucide-react";
import { Select } from "./ui";

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
      <Select value={year} onChange={(y) => emit(y, month)} ariaLabel="年">
        <option value="">年</option>
        {years.map((y) => (
          <option key={y} value={y}>
            {y} 年
          </option>
        ))}
      </Select>
      <Select value={month} onChange={(m) => emit(year, m)} ariaLabel="月">
        <option value="">月</option>
        {months.map((m) => (
          <option key={m} value={m}>
            {Number(m)} 月
          </option>
        ))}
      </Select>
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
