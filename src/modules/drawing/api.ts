// Typed wrappers for Rust drawing_* commands (src-tauri/src/modules/drawing.rs) + React Query hooks.
import { invoke } from "@tauri-apps/api/core";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { errorText, useToast } from "@/shared/feedback";
import { KINDS_KEY } from "@/shared/kinds/api";
import type { StatusCode, StatusLabels } from "@/shared/status";

export type Course = {
  id: number;
  name: string;
  note: string | null;
  sortOrder: number;
  total: number;
  done: number;
  doing: number;
  createdAt: string;
  updatedAt: string;
};

export type Lesson = {
  id: number;
  courseId: number;
  code: string | null;
  kindId: number | null;
  title: string;
  status: StatusCode;
  finishedMonth: string | null;
  note: string | null;
  sortOrder: number;
  createdAt: string;
  updatedAt: string;
};

export type CoursePatch = { name?: string; note?: string | null };
/** Omitted key = unchanged; null = clear. Rust auto-fills finishedMonth on → done only if omitted. */
export type LessonPatch = Partial<Pick<Lesson, "code" | "kindId" | "title" | "status" | "finishedMonth" | "note">>;

export const DRAWING_STATUS: StatusLabels = { unmarked: "未开始", todo: "未开始", doing: "进行中", done: "已完成" };
/** No 待标记 in 随意画画. */
export const DRAWING_ORDER: StatusCode[] = ["todo", "doing", "done"];

const api = {
  courses: () => invoke<Course[]>("drawing_courses"),
  createCourse: (patch: CoursePatch) => invoke<Course>("drawing_course_create", { patch }),
  updateCourse: (v: { id: number; patch: CoursePatch }) => invoke<Course>("drawing_course_update", v),
  deleteCourse: (id: number) => invoke<void>("drawing_course_delete", { id }),
  lessons: (courseId: number) => invoke<Lesson[]>("drawing_lessons", { courseId }),
  createLesson: (v: { courseId: number; patch: LessonPatch }) => invoke<Lesson>("drawing_lesson_create", v),
  updateLesson: (v: { id: number; patch: LessonPatch }) => invoke<Lesson>("drawing_lesson_update", v),
  deleteLesson: (id: number) => invoke<void>("drawing_lesson_delete", { id }),
  reorderCourses: (ids: number[]) => invoke<void>("drawing_courses_reorder", { ids }),
  reorderLessons: (v: { courseId: number; ids: number[] }) => invoke<void>("drawing_lessons_reorder", v),
};

export const DRAWING_KEY = ["drawing"];

export const useCourses = () => useQuery({ queryKey: [...DRAWING_KEY, "courses"], queryFn: api.courses });

export const useLessons = (courseId: number | null) =>
  useQuery({
    queryKey: [...DRAWING_KEY, "lessons", courseId],
    queryFn: () => api.lessons(courseId!),
    enabled: courseId !== null,
  });

export function useDrawingMutations() {
  const qc = useQueryClient();
  const toast = useToast();
  const onSuccess = () => {
    void qc.invalidateQueries({ queryKey: DRAWING_KEY }); // lessons + course progress
    void qc.invalidateQueries({ queryKey: KINDS_KEY });
  };
  const onError = (e: unknown) => toast(errorText(e), "error");
  return {
    createCourse: useMutation({ mutationFn: api.createCourse, onSuccess }),
    updateCourse: useMutation({ mutationFn: api.updateCourse, onSuccess }),
    deleteCourse: useMutation({ mutationFn: api.deleteCourse, onSuccess }),
    createLesson: useMutation({ mutationFn: api.createLesson, onSuccess }),
    updateLesson: useMutation({ mutationFn: api.updateLesson, onSuccess }),
    /** Fire-and-forget inline edits (row toggles): errors surface as toasts. */
    quickUpdate: useMutation({ mutationFn: api.updateLesson, onSuccess, onError }),
    deleteLesson: useMutation({ mutationFn: api.deleteLesson, onSuccess }),
    reorderCourses: useMutation({ mutationFn: api.reorderCourses, onSuccess, onError }),
    reorderLessons: useMutation({ mutationFn: api.reorderLessons, onSuccess, onError }),
  };
}
