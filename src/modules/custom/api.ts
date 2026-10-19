// Typed wrappers for the commands of modules made from 五花八门 / 循序渐进 (custom.rs, lists.rs, progress.rs)
// + React Query hooks. A made module is a sidebar module with the key (= route) "list/<id>" / "progress/<id>".
import { useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { MODULES, setMadeModules, type ModuleDef } from "@/app/modules";
import { errorText, useToast } from "@/shared/feedback";
import { KINDS_KEY } from "@/shared/kinds/api";
import type { StatusCode } from "@/shared/status";
import { iconOf } from "./icons";

export type Template = "list" | "progress";
export type Custom = {
  id: number;
  template: Template;
  name: string;
  icon: string;
  /** 循序渐进: what a course / lesson is called (empty for 五花八门). */
  courseWord: string;
  lessonWord: string;
  key: string;
  count: number;
  createdAt: string;
  updatedAt: string;
};
export type CustomPatch = Partial<{ template: Template; name: string; icon: string; courseWord: string; lessonWord: string }>;

export const TEMPLATE_LABEL: Record<Template, string> = { list: "五花八门", progress: "循序渐进" };
export const TEMPLATE_COLOR: Record<Template, string> = { list: "var(--module-list)", progress: "var(--module-progress)" };

// ---- 五花八门 ----
export type FieldType = "text" | "longtext" | "number" | "year" | "select" | "multi" | "check" | "rating" | "link";
export const FIELD_TYPES: { value: FieldType; label: string }[] = [
  { value: "text", label: "文字" },
  { value: "longtext", label: "长文字" },
  { value: "number", label: "数字" },
  { value: "year", label: "年份" },
  { value: "select", label: "单选" },
  { value: "multi", label: "多选" },
  { value: "check", label: "勾选" },
  { value: "rating", label: "评分" },
  { value: "link", label: "链接" },
];
export const fieldTypeLabel = (t: FieldType) => FIELD_TYPES.find((x) => x.value === t)!.label;
/** Same as MAX_FIELDS / MAX_OPTIONS in lists.rs. */
export const MAX_FIELDS = 20;
export const MAX_OPTIONS = 50;
export type Field = { id: number; moduleId: number; name: string; type: FieldType; options: string[]; shown: boolean; sortOrder: number };
/** options: `was` = the name before (renames carry into every item); left out = removed. */
export type FieldPatch = Partial<{ name: string; type: FieldType; options: { name: string; was?: string | null }[]; shown: boolean }>;
export type Value = string | number | boolean | string[];
export type Item = { id: number; moduleId: number; name: string; values: Record<string, Value>; sortOrder: number; createdAt: string; updatedAt: string };
/** values: only the fields given change (null empties one). */
export type ItemPatch = Partial<{ name: string; values: Record<string, Value | null> }>;

// ---- 循序渐进 ----
export type Course = { id: number; moduleId: number; name: string; note: string | null; sortOrder: number; total: number; done: number; doing: number; createdAt: string; updatedAt: string };
export type CoursePatch = Partial<{ name: string; note: string | null }>;
export type Lesson = { id: number; courseId: number; code: string | null; kindId: number | null; title: string; status: StatusCode; finishedMonth: string | null; note: string | null; sortOrder: number; createdAt: string; updatedAt: string };
export type LessonPatch = Partial<{ code: string | null; kindId: number | null; title: string; status: StatusCode; finishedMonth: string | null; note: string | null }>;

export const customApi = {
  list: () => invoke<Custom[]>("custom_list"),
  create: (patch: CustomPatch) => invoke<Custom>("custom_create", { patch }),
  update: (v: { id: number; patch: CustomPatch }) => invoke<Custom>("custom_update", v),
  remove: (id: number) => invoke<void>("custom_delete", { id }),
  fields: (moduleId: number) => invoke<Field[]>("list_fields", { moduleId }),
  fieldCreate: (v: { moduleId: number; patch: FieldPatch }) => invoke<Field>("list_field_create", v),
  fieldUpdate: (v: { id: number; patch: FieldPatch }) => invoke<Field>("list_field_update", v),
  fieldDelete: (id: number) => invoke<void>("list_field_delete", { id }),
  fieldsReorder: (v: { moduleId: number; ids: number[] }) => invoke<void>("list_fields_reorder", v),
  items: (moduleId: number) => invoke<Item[]>("list_items", { moduleId }),
  itemCreate: (v: { moduleId: number; patch: ItemPatch }) => invoke<Item>("list_item_create", v),
  itemUpdate: (v: { id: number; patch: ItemPatch }) => invoke<Item>("list_item_update", v),
  itemDelete: (id: number) => invoke<void>("list_item_delete", { id }),
  itemsReorder: (v: { moduleId: number; ids: number[] }) => invoke<void>("list_items_reorder", v),
  courses: (moduleId: number) => invoke<Course[]>("progress_courses", { moduleId }),
  courseCreate: (v: { moduleId: number; patch: CoursePatch }) => invoke<Course>("progress_course_create", v),
  courseUpdate: (v: { id: number; patch: CoursePatch }) => invoke<Course>("progress_course_update", v),
  courseDelete: (id: number) => invoke<void>("progress_course_delete", { id }),
  coursesReorder: (v: { moduleId: number; ids: number[] }) => invoke<void>("progress_courses_reorder", v),
  lessons: (courseId: number) => invoke<Lesson[]>("progress_lessons", { courseId }),
  lessonCreate: (v: { courseId: number; patch: LessonPatch }) => invoke<Lesson>("progress_lesson_create", v),
  lessonUpdate: (v: { id: number; patch: LessonPatch }) => invoke<Lesson>("progress_lesson_update", v),
  lessonDelete: (id: number) => invoke<void>("progress_lesson_delete", { id }),
  lessonsReorder: (v: { courseId: number; ids: number[] }) => invoke<void>("progress_lessons_reorder", v),
};

export const CUSTOM_KEY = ["custom"];
export const useCustomModules = () => useQuery({ queryKey: [...CUSTOM_KEY, "modules"], queryFn: customApi.list });
export const useFields = (moduleId: number) => useQuery({ queryKey: [...CUSTOM_KEY, "fields", moduleId], queryFn: () => customApi.fields(moduleId) });
export const useItems = (moduleId: number) => useQuery({ queryKey: [...CUSTOM_KEY, "items", moduleId], queryFn: () => customApi.items(moduleId) });
export const useCourses = (moduleId: number) => useQuery({ queryKey: [...CUSTOM_KEY, "courses", moduleId], queryFn: () => customApi.courses(moduleId) });
export const useLessons = (courseId: number | null) =>
  useQuery({ queryKey: [...CUSTOM_KEY, "lessons", courseId], queryFn: () => customApi.lessons(courseId!), enabled: courseId !== null });

export const madeDef = (c: Custom): ModuleDef => ({ id: c.key, label: c.name, icon: iconOf(c.icon), color: TEMPLATE_COLOR[c.template] });

/** Built-in modules, then the made ones (oldest first); also keeps moduleById / moduleLabel aware of them. */
export function useAllModules(): ModuleDef[] {
  const { data } = useCustomModules();
  return useMemo(() => {
    const made = (data ?? []).map(madeDef);
    setMadeModules(made);
    return [...MODULES, ...made];
  }, [data]);
}

export function useCustomMutations() {
  const qc = useQueryClient();
  const toast = useToast();
  const onSuccess = () => void qc.invalidateQueries({ queryKey: CUSTOM_KEY });
  const onError = (e: unknown) => toast(errorText(e), "error");
  // a lesson's 类型 changes what 管理类型 counts; a deleted module takes its 类型 list
  const withKinds = () => {
    onSuccess();
    void qc.invalidateQueries({ queryKey: KINDS_KEY });
  };
  return {
    create: useMutation({ mutationFn: customApi.create, onSuccess }),
    update: useMutation({ mutationFn: customApi.update, onSuccess }),
    remove: useMutation({ mutationFn: customApi.remove, onSuccess: withKinds, onError }),
    fieldCreate: useMutation({ mutationFn: customApi.fieldCreate, onSuccess }),
    fieldUpdate: useMutation({ mutationFn: customApi.fieldUpdate, onSuccess }),
    fieldDelete: useMutation({ mutationFn: customApi.fieldDelete, onSuccess, onError }),
    fieldsReorder: useMutation({ mutationFn: customApi.fieldsReorder, onSettled: onSuccess, onError }),
    itemCreate: useMutation({ mutationFn: customApi.itemCreate, onSuccess }),
    itemUpdate: useMutation({ mutationFn: customApi.itemUpdate, onSuccess, onError }),
    itemDelete: useMutation({ mutationFn: customApi.itemDelete, onSuccess, onError }),
    itemsReorder: useMutation({ mutationFn: customApi.itemsReorder, onSettled: onSuccess, onError }),
    courseCreate: useMutation({ mutationFn: customApi.courseCreate, onSuccess }),
    courseUpdate: useMutation({ mutationFn: customApi.courseUpdate, onSuccess }),
    courseDelete: useMutation({ mutationFn: customApi.courseDelete, onSuccess: withKinds, onError }),
    coursesReorder: useMutation({ mutationFn: customApi.coursesReorder, onSettled: onSuccess, onError }),
    lessonCreate: useMutation({ mutationFn: customApi.lessonCreate, onSuccess: withKinds }),
    lessonUpdate: useMutation({ mutationFn: customApi.lessonUpdate, onSuccess: withKinds, onError }),
    lessonDelete: useMutation({ mutationFn: customApi.lessonDelete, onSuccess: withKinds, onError }),
    lessonsReorder: useMutation({ mutationFn: customApi.lessonsReorder, onSettled: onSuccess, onError }),
  };
}
