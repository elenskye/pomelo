// Create / edit / delete a course.
import { useToast } from "@/shared/feedback";
import { NameDialog } from "@/shared/NameDialog";
import { useDrawingMutations, type Course } from "./api";

export function CourseDialog(props: {
  course: Course | null; // null = create
  onClose: () => void;
  onCreated: (c: Course) => void;
  onDeleted: () => void;
}) {
  const { course } = props;
  const m = useDrawingMutations();
  const toast = useToast();
  return (
    <NameDialog
      title={course ? "编辑课程" : "新课程"}
      nameLabel="课程名"
      initial={course}
      onClose={props.onClose}
      onSubmit={async (patch) => {
        if (course) {
          await m.updateCourse.mutateAsync({ id: course.id, patch });
          toast("已保存");
        } else {
          const c = await m.createCourse.mutateAsync(patch);
          toast(`已添加课程「${c.name}」`);
          props.onCreated(c);
        }
      }}
      remove={
        course ? {
          label: "删除课程",
          confirmTitle: `删除课程「${course.name}」？`,
          confirmMessage: `其中的 ${course.total} 节课也会一起删除。删错了可以按 ⌘Z 撤销。`,
          run: async () => {
            await m.deleteCourse.mutateAsync(course.id);
            toast(`已删除课程「${course.name}」`);
            props.onDeleted();
          },
        } : undefined
      }
    />
  );
}
