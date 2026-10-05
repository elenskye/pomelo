// Shared bits of the 一步一步 right pane: view header (title, sub line, edit, actions) and a titled section.
import type { ReactNode } from "react";
import { Pencil } from "lucide-react";
import { IconButton } from "@/shared/ui";

export function ViewHeader(props: { title: ReactNode; sub?: ReactNode; onEdit?: () => void; editLabel?: string; actions?: ReactNode }) {
  return (
    <div className="flex shrink-0 items-start justify-between gap-4 pb-3">
      <div className="min-w-0">
        <div className="flex items-center gap-1">
          <h2 className="truncate text-lg font-bold">{props.title}</h2>
          {props.onEdit && (
            <IconButton label={props.editLabel ?? "编辑"} onClick={props.onEdit}>
              <Pencil size={14} />
            </IconButton>
          )}
        </div>
        {props.sub && <div className="text-sm text-muted-foreground">{props.sub}</div>}
      </div>
      {props.actions && <div className="flex shrink-0 gap-2">{props.actions}</div>}
    </div>
  );
}

export function Section(props: { title: ReactNode; danger?: boolean; children: ReactNode }) {
  return (
    <section>
      <h3 className={`mb-1.5 px-1 text-sm ${props.danger ? "text-destructive" : "text-muted-foreground"}`}>{props.title}</h3>
      {props.children}
    </section>
  );
}
