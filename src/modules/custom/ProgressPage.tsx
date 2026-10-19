// 循序渐进: one made progress module (route progress/:id), like 随意画画 in its own words (课程 / 课节 by
// default). Left: the courses with progress (drag to order); right: the chosen one's lessons (circle =
// done toggle, 类型 as a colored tag that switches in place, status pill, drag to order) with 下一节;
// filters by status and 类型. The 类型 list is the module's own (kinds "progress/<id>").
// Header ⋯: 改名和叫法 · 管理类型 · 删除.
import { useMemo, useState } from "react";
import { Navigate, useParams } from "react-router";
import { CheckCircle2, Circle, CircleDot, Ellipsis, Pencil, Plus } from "lucide-react";
import { usePageContext } from "@/app/pageContext";
import { Dropdown } from "@/shared/Dropdown";
import { useKinds, UNCATEGORIZED, type Kind } from "@/shared/kinds/api";
import { KindManager } from "@/shared/kinds/KindManager";
import { useConfirm, useToast } from "@/shared/feedback";
import { NameDialog } from "@/shared/NameDialog";
import { useOpenRequest } from "@/shared/open";
import { usePageKeys, usePanelSelection } from "@/shared/panel";
import { DragHandle, useSortable } from "@/shared/sortable";
import { StatusSelect, type StatusCode } from "@/shared/status";
import { TagSelect } from "@/shared/tags";
import { Button, EmptyState, IconButton, PageHeader, ProgressBar, Segmented } from "@/shared/ui";
import { CUSTOM_KEY, madeDef, TEMPLATE_COLOR, useCourses, useCustomModules, useCustomMutations, useLessons, type Course, type Custom, type Lesson } from "./api";
import { CustomDialog } from "./CustomDialog";
import { LessonPanel, PROGRESS_ORDER, PROGRESS_STATUS } from "./LessonPanel";

type Filter = "all" | StatusCode;
type KindFilter = "all" | "none" | number;
const pct = (done: number, total: number) => (total > 0 ? Math.round((done / total) * 100) : 0);

function CourseList(props: { moduleId: number; word: string; courses: Course[]; currentId: number | null; onSelect: (id: number) => void; onNew: () => void }) {
  const m = useCustomMutations();
  const sort = useSortable((ids) => m.coursesReorder.mutate({ moduleId: props.moduleId, ids }));
  return (
    <nav aria-label={props.word} className="flex w-56 shrink-0 flex-col">
      <div ref={sort.listRef} className="-m-1 min-h-0 flex-1 space-y-1 overflow-y-auto p-1 pr-2">
        {props.courses.map((c) => (
          <button
            key={c.id}
            type="button"
            aria-current={c.id === props.currentId}
            onClick={() => props.onSelect(c.id)}
            {...sort.item(c.id)}
            className={`group w-full space-y-1.5 rounded-control px-3 py-2.5 text-left transition-colors duration-150 outline-none focus-visible:ring-2 focus-visible:ring-ring ${c.id === props.currentId ? "bg-accent" : "bg-background hover:bg-muted/70"}`}
          >
            <div className="flex items-baseline justify-between gap-2">
              <span className="-ml-2 flex min-w-0 items-center gap-0.5">
                {props.courses.length > 1 && <DragHandle {...sort.handle(c.id)} />}
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
        <Plus size={14} /> 新{props.word}
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

function LessonTable(props: { lessons: Lesson[]; kinds: Kind[]; selectedId: number | null; onSelect: (id: number) => void; onReorder: (ids: number[]) => void }) {
  const m = useCustomMutations();
  const sort = useSortable<HTMLTableSectionElement>(props.onReorder);
  const muted = "text-muted-foreground";
  return (
    <div className="rounded-card border bg-card">
      <table className="w-full table-fixed text-sm">
        <thead>
          <tr className="border-b text-left text-xs text-muted-foreground">
            <th className="w-[18px]" aria-label="拖动" />
            <th className="w-[36px] py-2" aria-label="完成" />
            <th className="w-[56px] px-2 py-2 font-normal">编号</th>
            <th className="px-3 py-2 font-normal">名称</th>
            <th className="w-[112px] px-3 py-2 font-normal">类型</th>
            <th className="w-[88px] px-3 py-2 font-normal">状态</th>
            <th className="w-[92px] px-3 py-2 font-normal max-lg:hidden">完成</th>
            <th className="w-[24%] px-3 py-2 font-normal max-xl:hidden">备注</th>
          </tr>
        </thead>
        <tbody ref={sort.listRef}>
          {props.lessons.map((l) => (
            <tr
              key={l.id}
              onClick={() => props.onSelect(l.id)}
              {...sort.item(l.id)}
              className={`group h-10 cursor-pointer border-b transition-colors duration-150 last:border-b-0 ${l.id === props.selectedId ? "bg-accent" : "bg-card hover:bg-muted/70"}`}
            >
              <td className="pl-1">
                <DragHandle {...sort.handle(l.id)} />
              </td>
              <td>
                <DoneToggle lesson={l} onToggle={() => m.lessonUpdate.mutate({ id: l.id, patch: l.status === "done" ? { status: "todo", finishedMonth: null } : { status: "done" } })} />
              </td>
              <td className={`truncate px-2 ${muted}`}>{l.code ?? ""}</td>
              <td className={`truncate px-3 ${l.status === "done" ? muted : ""}`}>{l.title}</td>
              <td className="px-3">
                <TagSelect
                  label="类型"
                  options={props.kinds.map((k) => ({ value: k.id, name: k.name }))}
                  value={l.kindId}
                  onChange={(kindId) => m.lessonUpdate.mutate({ id: l.id, patch: { kindId } })}
                />
              </td>
              <td className="px-3">
                <StatusSelect value={l.status} labels={PROGRESS_STATUS} order={PROGRESS_ORDER} onChange={(status) => m.lessonUpdate.mutate({ id: l.id, patch: { status } })} />
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

function ProgressView(props: { mod: Custom }) {
  const mod = props.mod;
  const def = madeDef(mod);
  const [cw, lw] = [mod.courseWord, mod.lessonWord];
  const { data: courses = [], isPending } = useCourses(mod.id);
  const [courseId, setCourseId] = useState<number | null>(null);
  const current = courses.find((c) => c.id === courseId) ?? courses[0] ?? null;
  const lessonsQuery = useLessons(current?.id ?? null);
  const lessons = useMemo(() => lessonsQuery.data ?? [], [lessonsQuery.data]);
  const [filter, setFilter] = useState<Filter>("all");
  const kindsModule = `progress/${mod.id}`;
  const { data: kinds = [] } = useKinds(kindsModule);
  const [kind, setKind] = useState<KindFilter>("all");
  const [managingKinds, setManagingKinds] = useState(false);
  const [courseDialog, setCourseDialog] = useState<"new" | "edit" | null>(null);
  const [renaming, setRenaming] = useState(false);
  const panel = usePanelSelection<number | "new">();
  const sel = panel.selection;
  const m = useCustomMutations();
  const toast = useToast();
  const [confirm, confirmDialog] = useConfirm();

  const totals = courses.reduce((a, c) => ({ total: a.total + c.total, done: a.done + c.done }), { total: 0, done: 0 });
  const counts = useMemo(() => {
    const c: Record<StatusCode, number> = { unmarked: 0, todo: 0, doing: 0, done: 0 };
    lessons.forEach((l) => c[l.status]++);
    return c;
  }, [lessons]);
  const uncategorized = lessons.filter((l) => l.kindId === null).length;
  const kindFilter: KindFilter = (typeof kind === "number" && !kinds.some((k) => k.id === kind)) || (kind === "none" && uncategorized === 0) ? "all" : kind;
  const kindName = (k: KindFilter) => (k === "all" ? null : k === "none" ? UNCATEGORIZED : (kinds.find((x) => x.id === k)?.name ?? null));
  const visible = lessons.filter((l) => (filter === "all" || l.status === filter) && (kindFilter === "all" || (kindFilter === "none" ? l.kindId === null : l.kindId === kindFilter)));
  const next = lessons.find((l) => l.status === "doing") ?? lessons.find((l) => l.status !== "done");
  const selectedLesson = typeof sel === "number" ? (lessons.find((l) => l.id === sel) ?? null) : null;
  usePageContext({
    module: mod.key,
    page: current?.name ?? null,
    filter: [filter === "all" ? null : PROGRESS_STATUS[filter], kindName(kindFilter)].filter(Boolean).join(" · ") || null,
    record: selectedLesson
      ? { kind: "progress_lesson", id: selectedLesson.id, label: `${lw}：${[selectedLesson.code, selectedLesson.title].filter(Boolean).join(" ")}` }
      : current
        ? { kind: "progress_course", id: current.id, label: `${cw}：${current.name}` }
        : null,
  });

  const selectCourse = async (id: number) => {
    if (id === current?.id) return;
    if (await panel.request(null)) setCourseId(id);
  };
  useOpenRequest(async (h) => {
    if (!(await panel.request(null))) return;
    setFilter("all");
    setKind("all");
    setCourseId(h.kind === "progress_course" ? h.id : h.parent);
    if (h.kind === "progress_lesson") void panel.request(h.id);
  });
  usePageKeys({ onNew: current ? () => void panel.request("new") : undefined, onEscape: sel !== null ? () => void panel.request(null) : undefined });

  const del = async () => {
    const ok = await confirm({ title: `删除「${mod.name}」？`, message: courses.length > 0 ? `里面的 ${courses.length} 个${cw}和 ${totals.total} 个${lw}也会一起删掉。` : undefined, confirmLabel: "删除", danger: true });
    if (ok) m.remove.mutate(mod.id, { onSuccess: () => toast("已删除") });
  };

  const stats = (
    <div className="flex items-center justify-end gap-2">
      {courses.length > 0 && (
        <p className="pointer-events-none text-sm text-muted-foreground">
          共 <b className="text-base text-foreground">{courses.length}</b> 个{cw} · <b className="text-base text-foreground">{totals.total}</b> {lw} · 已完成{" "}
          <b className="text-base text-foreground">{totals.done}</b>（{pct(totals.done, totals.total)}%）
        </p>
      )}
      <Dropdown<string | null>
        plain
        ariaLabel="更多"
        value={null}
        align="end"
        onChange={(a) => (a === "rename" ? setRenaming(true) : a === "kinds" ? setManagingKinds(true) : a === "delete" && void del())}
        options={[
          { value: "rename", label: "改名、图标和叫法…" },
          { value: "kinds", label: "管理类型…" },
          { value: "delete", label: <span className="text-destructive">删除这个进度…</span> },
        ]}
        renderTrigger={() => <Ellipsis size={16} />}
        triggerClassName="rounded-control p-1.5 text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
      />
    </div>
  );

  return (
    <div className="relative flex h-full min-h-0 flex-col overflow-hidden">
      <PageHeader icon={def.icon} color={TEMPLATE_COLOR.progress} title={mod.name} right={stats} />
      {isPending ? null : courses.length === 0 ? (
        <EmptyState
          title="慢慢来，先建第一个吧"
          action={
            <Button variant="primary" onClick={() => setCourseDialog("new")}>
              <Plus size={14} /> 新{cw}
            </Button>
          }
        />
      ) : (
        <div className="flex min-h-0 flex-1 gap-6 px-8 pb-8">
          <CourseList moduleId={mod.id} word={cw} courses={courses} currentId={current?.id ?? null} onSelect={(id) => void selectCourse(id)} onNew={() => setCourseDialog("new")} />
          {current && (
            <section className="flex min-w-0 flex-1 flex-col">
              <div className="flex shrink-0 items-start justify-between gap-4 pb-3">
                <div className="min-w-0">
                  <div className="flex items-center gap-1">
                    <h2 className="truncate text-lg font-bold">{current.name}</h2>
                    <IconButton label={`编辑${cw}`} onClick={() => setCourseDialog("edit")}>
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
                          {` · 下一${lw}：`}
                          <button type="button" className="text-foreground underline-offset-4 hover:underline" onClick={() => void panel.request(next.id)}>
                            {[next.code, next.title].filter(Boolean).join(" ")}
                          </button>
                        </>
                      ) : (
                        " · 全部完成了"
                      ))}
                  </p>
                </div>
                <Button variant="primary" onClick={() => void panel.request("new")}>
                  <Plus size={14} /> 新{lw}
                </Button>
              </div>
              {lessons.length > 0 && (
                <div className="flex shrink-0 items-center gap-2 pb-3">
                  <Segmented
                    ariaLabel="按状态筛选"
                    value={filter}
                    onChange={setFilter}
                    options={[{ value: "all" as Filter, label: `全部 ${lessons.length}` }, ...PROGRESS_ORDER.map((s) => ({ value: s as Filter, label: `${PROGRESS_STATUS[s]} ${counts[s]}` }))]}
                  />
                  {kinds.length > 0 && (
                    <Dropdown<KindFilter>
                      ariaLabel="按类型筛选"
                      value={kindFilter}
                      onChange={setKind}
                      options={[
                        { value: "all" as KindFilter, label: "全部类型" },
                        ...kinds.map((k) => ({ value: k.id as KindFilter, label: k.name })),
                        ...(uncategorized > 0 ? [{ value: "none" as KindFilter, label: UNCATEGORIZED }] : []),
                      ]}
                      actions={[{ label: "管理类型…", onSelect: () => setManagingKinds(true) }]}
                    />
                  )}
                </div>
              )}
              <div className="-m-1 flex min-h-0 flex-1 flex-col overflow-y-auto p-1">
                {lessonsQuery.isPending ? null : lessons.length === 0 ? (
                  <EmptyState
                    title={`还没有${lw}`}
                    action={
                      <Button variant="primary" onClick={() => void panel.request("new")}>
                        <Plus size={14} /> 新{lw}
                      </Button>
                    }
                  />
                ) : visible.length === 0 ? (
                  <EmptyState title="没有符合条件的" />
                ) : (
                  <LessonTable lessons={visible} kinds={kinds} selectedId={typeof sel === "number" ? sel : null} onSelect={(id) => void panel.request(id)} onReorder={(ids) => m.lessonsReorder.mutate({ courseId: current.id, ids })} />
                )}
              </div>
            </section>
          )}
        </div>
      )}
      {current && sel !== null && (sel === "new" || selectedLesson) && (
        <LessonPanel key={sel} courseId={current.id} word={lw} lesson={selectedLesson} kinds={kinds} onManageKinds={() => setManagingKinds(true)} onClose={() => void panel.request(null)} onDone={panel.done} onDirtyChange={panel.setDirty} />
      )}
      {courseDialog && (
        <NameDialog
          title={courseDialog === "edit" ? `编辑${cw}` : `新${cw}`}
          nameLabel="名称"
          initial={courseDialog === "edit" ? current : null}
          onClose={() => setCourseDialog(null)}
          onSubmit={async (patch) => {
            if (courseDialog === "edit" && current) {
              await m.courseUpdate.mutateAsync({ id: current.id, patch });
              toast("已保存");
            } else {
              const c = await m.courseCreate.mutateAsync({ moduleId: mod.id, patch });
              toast(`已添加「${c.name}」`);
              setCourseId(c.id);
            }
          }}
          remove={
            courseDialog === "edit" && current
              ? {
                  label: `删除${cw}`,
                  confirmTitle: `删除「${current.name}」？`,
                  confirmMessage: `其中的 ${current.total} 个${lw}也会一起删掉。`,
                  run: async () => {
                    await m.courseDelete.mutateAsync(current.id);
                    toast("已删除");
                    setCourseId(null);
                  },
                }
              : undefined
          }
        />
      )}
      {renaming && <CustomDialog template="progress" custom={mod} onClose={() => setRenaming(false)} />}
      {managingKinds && <KindManager module={kindsModule} moduleLabel={mod.name} unit={lw} itemsKey={CUSTOM_KEY} onClose={() => setManagingKinds(false)} />}
      {panel.confirmDialog}
      {confirmDialog}
    </div>
  );
}

export default function ProgressPage() {
  const { id } = useParams();
  const { data, isSuccess } = useCustomModules();
  const mod = data?.find((c) => c.template === "progress" && c.id === Number(id));
  if (!mod) return isSuccess ? <Navigate to="/" replace /> : null;
  return <ProgressView key={mod.id} mod={mod} />;
}
