// UI preferences kept in Rust's settings table (outside undo): useSetting(key, fallback) → [value, set].
// The value starts as `fallback` until the stored one loads.
import { useEffect, useState } from "react";
import { api } from "./ipc";

export function useSetting<T>(key: string, fallback: T, valid: (v: unknown) => v is T) {
  const [value, setValue] = useState<T>(fallback);
  useEffect(() => {
    let live = true;
    api
      .settingsGet<unknown>(key)
      .then((v) => live && valid(v) && setValue(v))
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [key]); // eslint-disable-line react-hooks/exhaustive-deps
  const set = (v: T) => {
    setValue(v);
    api.settingsSet(key, v).catch(() => {});
  };
  return [value, set] as const;
}

export const isNumberArray = (v: unknown): v is number[] => Array.isArray(v) && v.every((x) => typeof x === "number");
export const isStringArray = (v: unknown): v is string[] => Array.isArray(v) && v.every((x) => typeof x === "string");
export const isBoolean = (v: unknown): v is boolean => typeof v === "boolean";
