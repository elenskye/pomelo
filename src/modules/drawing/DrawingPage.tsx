import { moduleById } from "@/app/modules";
import { EmptyState, PageHeader } from "@/shared/ui";

export default function DrawingPage() {
  const m = moduleById("drawing");
  return (
    <div className="flex h-full flex-col">
      <PageHeader icon={m.icon} color={m.color} title={m.label} />
      <EmptyState title="画板空空，随手画一笔吧" hint="绘画进度将在阶段 4 上线" />
    </div>
  );
}
