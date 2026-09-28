import { moduleById } from "@/app/modules";
import { EmptyState, PageHeader } from "@/shared/ui";

export default function PackingPage() {
  const m = moduleById("packing");
  return (
    <div className="flex h-full flex-col">
      <PageHeader icon={m.icon} color={m.color} title={m.label} />
      <EmptyState title="行李箱空空，说走就走" hint="行李清单将在阶段 5 上线" />
    </div>
  );
}
