import { moduleById } from "@/app/modules";
import { EmptyState, PageHeader } from "@/shared/ui";

export default function FilmsPage() {
  const m = moduleById("films");
  return (
    <div className="flex h-full flex-col">
      <PageHeader icon={m.icon} color={m.color} title={m.label} />
      <EmptyState title="片单空空，先去泡杯茶" hint="影单功能将在阶段 3 上线" />
    </div>
  );
}
