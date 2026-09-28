import { moduleById } from "@/app/modules";
import { EmptyState, PageHeader } from "@/shared/ui";

export default function BooksPage() {
  const m = moduleById("books");
  return (
    <div className="flex h-full flex-col">
      <PageHeader icon={m.icon} color={m.color} title={m.label} />
      <EmptyState title="书架空空，去摸会儿鱼吧" hint="书单功能将在阶段 2 上线" />
    </div>
  );
}
