import { useMemo, useState } from "react";
import { CheckCircle2, Circle, CircleDot, Pencil, Plus } from "lucide-react";
import { moduleById } from "@/app/modules";
import { Dropdown } from "@/shared/Dropdown";
import { useKinds, UNCATEGORIZED } from "@/shared/kinds/api";
import { KindManager } from "@/shared/kinds/KindManager";
import { useOpenRequest } from "@/shared/open";
import { usePageKeys, usePanelSelection } from "@/shared/panel";
import { DragHandle, useSortable } from "@/shared/sortable";
import { StatusSelect, type StatusCode } from "@/shared/status";
import { Button, EmptyState, IconButton, PageHeader, ProgressBar, Segmented } from "@/shared/ui";
import { DRAWING_KEY, DRAWING_ORDER, DRAWING_STATUS, useCourses, useDrawingMutations, useLessons, type Course, type Lesson } from "./api";
import { CourseDialog } from "./CourseDialog";
import { LessonPanel } from "./LessonPanel";
import { usePageContext } from "@/app/pageContext";

type Filter = "all" | StatusCode;
type KindFilter = "all" | "none" | number;

const pct = (done: number, total: number) => (total > 0 ? Math.round((done / total) * 100) : 0);

function CourseList(props: { courses: Course[]; currentId: number | null; onSelect: (id: number) => void; onNew: () => void }) {
  const { reorderCourses } = useDrawingMutations();
  const sort = useSortable((ids) => reorderCourses.mutate(ids));
  return (
    <nav aria-label="课程" className="flex w-56 shrink-0 flex-col">
      <div ref={sort.listRef} className="-m-1 min-h-0 flex-1 space-y-1 overflow-y-auto p-1 pr-2">
        {props.courses.map((c) => (
          <button
            key={c.id}
            type="button"
            aria-current={c.id === props.currentId}
            onClick={() => props.onSelect(c.id)}
            {...sort.item(c.id)}
            className={`group w-full space-y-1.5 rounded-control px-3 py-2.5 text-left transition-colors duration-150 outline-none focus-visible:ring-2 focus-visible:ring-ring ${
              c.id === props.currentId ? "bg-accent" : "bg-background hover:bg-muted/70"
            }`}
          >
            <div className="flex items-baseline justify-between gap-2">
              <span className="-ml-2 flex min-w-0 items-center gap-0.5">
                {props.courses.length > 1 && <DragHandle {...sort.handle(c.id)} label="拖动课程排序" />}
                <span className="truncate">{c.name}</span>
              </span>
              <span className="shrink-0 text-xs text-muted-foreground">
                {c.done}/{c.total}
              </span>
            </div>
            <ProgressBar done={c.done} doing={c.doing} total={c.total} />
          </button>
        ))}
      </div>
      <Button variant="ghost" className="mt-2 justify-center text-muted-foreground" onClick={props.onNew}>
        <Plus size={14} /> 新课程
      </Button>
    </nav>
  );
}

function DoneToggle(props: { lesson: Lesson; onToggle: () => void }) {
  const s = props.lesson.status;
  const Icon = s === "done" ? CheckCircle2 : s === "doing" ? CircleDot : Circle;
  const color = s === "done" ? "var(--status-done-fg)" : s === "doing" ? "var(--status-doing-fg)" : "var(--muted-foreground)";
  return (
    <button
      type="button"
      aria-label={s === "done" ? `把「${props.lesson.title}」标为未开始` : `把「${props.lesson.title}」标为已完成`}
      onClick={(e) => {
        e.stopPropagation();
        props.onToggle();
      }}
      className="rounded-full p-1 outline-none transition-transform duration-150 hover:scale-110 focus-visible:ring-2 focus-visible:ring-ring"
    >
      <Icon size={18} strokeWidth={1.75} style={{ color }} />
    </button>
  );
}

function LessonTable(props: {
  lessons: Lesson[];
  kindName: Map<number, string>;
  selectedId: number | null;
  onSelect: (id: number) => void;
  /** Rows can be dragged (also when filtered: hidden lessons keep their places). */
  onReorder?: (ids: number[]) => void;
}) {
  const { quickUpdate } = useDrawingMutations();
  const sort = useSortable<HTMLTableSectionElement>((ids) => props.onReorder?.(ids));
  const muted = "text-muted-foreground";
  return (
    <div className="rounded-card border bg-card">
      <table className="w-full table-fixed text-sm">
        <thead>
          <tr className="border-b text-left text-xs text-muted-foreground">
            <th className="w-[18px]" aria-label="拖动" />
            <th className="w-[36px] py-2" aria-label="完成" />
            <th className="w-[56px] px-2 py-2 font-normal">编号</th>
            <th className="px-3 py-2 font-normal">课名</th>
            <th className="w-[96px] px-3 py-2 font-normal max-lg:hidden">类型</th>
            <th className="w-[88px] px-3 py-2 font-normal">状态</th>
            <th className="w-[92px] px-3 py-2 font-normal max-lg:hidden">完成</th>
            <th className="w-[22%] px-3 py-2 font-normal max-xl:hidden">备注</th>
          </tr>
        </thead>
        <tbody ref={sort.listRef}>
          {props.lessons.map((l) => (
            <tr
              key={l.id}
              onClick={() => props.onSelect(l.id)}
              {...sort.item(l.id)}
              className={`group h-10 cursor-pointer border-b transition-colors duration-150 last:border-b-0 ${
                l.id === props.selectedId ? "bg-accent" : "bg-card hover:bg-muted/70"
              }`}
            >
              <td className="pl-1">{props.onReorder && <DragHandle {...sort.handle(l.id)} label="拖动课节排序" />}</td>
              <td>
                <DoneToggle
                  lesson={l}
                  onToggle={() =>
                    quickUpdate.mutate({
                      id: l.id,
                      // un-complete = undo: clear the month too; complete: Rust auto-fills this month
                      patch: l.status === "done" ? { status: "todo", finishedMonth: null } : { status: "done" },
                    })
                  }
                />
              </td>
              <td className={`truncate px-2 ${muted}`}>{l.code ?? ""}</td>
              <td className={`truncate px-3 ${l.status === "done" ? muted : ""}`}>{l.title}</td>
              <td className={`truncate px-3 max-lg:hidden ${muted}`}>{(l.kindId !== null && props.kindName.get(l.kindId)) || "—"}</td>
              <td className="px-3">
                <StatusSelect
                  value={l.status}
                  labels={DRAWING_STATUS}
                  order={DRAWING_ORDER}
                  onChange={(status) => quickUpdate.mutate({ id: l.id, patch: { status } })}
                />
              </td>
              <td className={`px-3 whitespace-nowrap max-lg:hidden ${muted}`}>{l.finishedMonth ?? "—"}</td>
              <td className={`truncate px-3 max-xl:hidden ${muted}`}>{l.note ?? ""}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

export default function DrawingPage() {
  const m = moduleById("drawing");
  const { data: courses = [], isPending } = useCourses();
  const [courseId, setCourseId] = useState<number | null>(null);
  const current = courses.find((c) => c.id === courseId) ?? courses[0] ?? null;
  const lessonsQuery = useLessons(current?.id ?? null);
  const lessons = useMemo(() => lessonsQuery.data ?? [], [lessonsQuery.data]);
  const { data: kinds = [] } = useKinds("drawing");
  const [filter, setFilter] = useState<Filter>("all");
  const [kind, setKind] = useState<KindFilter>("all");
  const [courseDialog, setCourseDialog] = useState<"new" | "edit" | null>(null);
  const [managingKinds, setManagingKinds] = useState(false);
  const panel = usePanelSelection<number | "new">();
  const sel = panel.selection;
  const { reorderLessons } = useDrawingMutations();

  const kindName = useMemo(() => new Map(kinds.map((k) => [k.id, k.name])), [kinds]);
  const totals = useMemo(
    () => courses.reduce((a, c) => ({ total: a.total + c.total, done: a.done + c.done }), { total: 0, done: 0 }),
    [courses],
  );
  const counts = useMemo(() => {
    const c: Record<StatusCode, number> = { unmarked: 0, todo: 0, doing: 0, done: 0 };
    lessons.forEach((l) => c[l.status]++);
    return c;
  }, [lessons]);
  const uncategorized = lessons.filter((l) => l.kindId === null).length;
  const kindFilter: KindFilter =
    (typeof kind === "number" && !kinds.some((k) => k.id === kind)) || (kind === "none" && uncategorized === 0) ? "all" : kind;
  const visible = lessons.filter(
    (l) =>
      (filter === "all" || l.status === filter) &&
      (kindFilter === "all" || (kindFilter === "none" ? l.kindId === null : l.kindId === kindFilter)),
  );
  const next = lessons.find((l) => l.status !== "done");
  const selectedLesson = typeof sel === "number" ? (lessons.find((l) => l.id === sel) ?? null) : null;
  usePageContext({
    module: "drawing",
    page: current?.name ?? null,
    filter: [filter === "all" ? null : DRAWING_STATUS[filter], kindFilter === "all" ? null : kindFilter === "none" ? "未分类" : (kindName.get(kindFilter) ?? null)].filter(Boolean).join(" · ") || null,
    record: selectedLesson
      ? { kind: "lesson", id: selectedLesson.id, label: `课节：${selectedLesson.code ? `${selectedLesson.code} ` : ""}${selectedLesson.title}` }
      : current
        ? { kind: "course", id: current.id, label: `课程：${current.name}` }
        : null,
  });

  const selectCourse = async (id: number) => {
    if (id === current?.id) return;
    if (await panel.request(null)) setCourseId(id);
  };

  // 全局搜索: a course, or a lesson (its course first, then its panel)
  useOpenRequest(async (h) => {
    if (!(await panel.request(null))) return;
    setFilter("all");
    setKind("all");
    setCourseId(h.kind === "course" ? h.id : h.parent);
    if (h.kind === "lesson") void panel.request(h.id);
  });
  usePageKeys({
    onNew: current ? () => void panel.request("new") : undefined,
    onEscape: sel !== null ? () => void panel.request(null) : undefined,
  });

  const stats = courses.length > 0 && (
    <p className="pointer-events-none text-sm text-muted-foreground">
      共 <b className="text-base text-foreground">{courses.length}</b> 门课 · <b className="text-base text-foreground">{totals.total}</b> 节 ·
      已完成 <b className="text-base text-foreground">{totals.done}</b>（{pct(totals.done, totals.total)}%）
    </p>
  );

  return (
    <div className="relative flex h-full min-h-0 flex-col overflow-hidden">
      <PageHeader icon={m.icon} color={m.color} title={m.label} right={stats} />

      {isPending ? null : courses.length === 0 ? (
        <EmptyState
          title="画板空空，随手画一笔吧"
          action={
            <Button variant="primary" onClick={() => setCourseDialog("new")}>
              <Plus size={14} /> 新课程
            </Button>
          }
        />
      ) : (
        <div className="flex min-h-0 flex-1 gap-6 px-8 pb-8">
          <CourseList
            courses={courses}
            currentId={current?.id ?? null}
            onSelect={(id) => void selectCourse(id)}
            onNew={() => setCourseDialog("new")}
          />

          {current && (
            <section className="flex min-w-0 flex-1 flex-col">
              <div className="flex shrink-0 items-start justify-between gap-4 pb-3">
                <div className="min-w-0">
                  <div className="flex items-center gap-1">
                    <h2 className="truncate text-lg font-bold">{current.name}</h2>
                    <IconButton label="编辑课程" onClick={() => setCourseDialog("edit")}>
                      <Pencil size={14} />
                    </IconButton>
                  </div>
                  <p className="text-sm text-muted-foreground">
                    已完成 {current.done} / {current.total}（{pct(current.done, current.total)}%）
                    {current.doing > 0 && ` · 进行中 ${current.doing}`}
                    {current.total > 0 &&
                      lessonsQuery.isSuccess &&
                      (next ? (
                        <>
                          {" · 下一课："}
                          <button
                            type="button"
                            className="text-foreground underline-offset-4 hover:underline"
                            onClick={() => void panel.request(next.id)}
                          >
                            {[next.code, next.title].filter(Boolean).join(" ")}
                          </button>
                        </>
                      ) : (
                        " · 这门课全部完成了"
                      ))}
                  </p>
                </div>
                <Button variant="primary" onClick={() => void panel.request("new")}>
                  <Plus size={14} /> 新课节
                </Button>
              </div>

              {lessons.length > 0 && (
                <div className="flex shrink-0 flex-wrap items-center gap-3 pb-3">
                  <Segmented
                    ariaLabel="按状态筛选"
                    value={filter}
                    onChange={setFilter}
                    options={[
                      { value: "all" as Filter, label: `全部 ${lessons.length}` },
                      ...DRAWING_ORDER.map((s) => ({ value: s as Filter, label: `${DRAWING_STATUS[s]} ${counts[s]}` })),
                    ]}
                  />
                  <div className="flex-1" />
                  <Dropdown<KindFilter>
                    ariaLabel="按类型筛选"
                    align="end"
                    value={kindFilter}
                    onChange={setKind}
                    options={[
                      { value: "all", label: "全部类型" },
                      ...kinds.map((k) => ({ value: k.id as KindFilter, label: k.name })),
                      ...(uncategorized > 0 ? [{ value: "none" as KindFilter, label: UNCATEGORIZED }] : []),
                    ]}
                    actions={[{ label: "管理类型…", onSelect: () => setManagingKinds(true) }]}
                  />
                </div>
              )}

              <div className="-m-1 flex min-h-0 flex-1 flex-col overflow-y-auto p-1">
                {lessonsQuery.isPending ? null : lessons.length === 0 ? (
                  <EmptyState
                    title="这门课还没有课节"
                    action={
                      <Button variant="primary" onClick={() => void panel.request("new")}>
                        <Plus size={14} /> 新课节
                      </Button>
                    }
                  />
                ) : visible.length === 0 ? (
                  <EmptyState title="没有符合条件的课节" />
                ) : (
                  <LessonTable
                    lessons={visible}
                    kindName={kindName}
                    selectedId={typeof sel === "number" ? sel : null}
                    onSelect={(id) => void panel.request(id)}
                    onReorder={
                      current ? (ids) => reorderLessons.mutate({ courseId: current.id, ids }) : undefined
                    }
                  />
                )}
              </div>
            </section>
          )}
        </div>
      )}

      {current && sel !== null && (sel === "new" || selectedLesson) && (
        <LessonPanel
          key={sel}
          courseId={current.id}
          lesson={selectedLesson}
          onClose={() => void panel.request(null)}
          onDone={panel.done}
          onDirtyChange={panel.setDirty}
          onManageKinds={() => setManagingKinds(true)}
        />
      )}
      {courseDialog && (
        <CourseDialog
          course={courseDialog === "edit" ? current : null}
          onClose={() => setCourseDialog(null)}
          onCreated={(c) => setCourseId(c.id)}
          onDeleted={() => setCourseId(null)}
        />
      )}
      {managingKinds && (
        <KindManager
          module="drawing"
          moduleLabel={m.label}
          unit="节"
          itemsKey={DRAWING_KEY}
          onClose={() => setManagingKinds(false)}
        />
      )}
      {panel.confirmDialog}
    </div>
  );
}
