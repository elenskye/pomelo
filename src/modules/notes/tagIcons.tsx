// Tag icons: one from the icon library (shared/iconLibrary.tsx, kept as "i:<key>"); no icon = #. An emoji typed
// before the library replaced the emoji box is still shown.
import { StoredIcon } from "@/shared/iconLibrary";

/** A tag's icon in a 16px slot: library icon (module color), an old emoji, or a muted #. */
export function TagIcon(props: { icon: string | null; size?: number }) {
  return <StoredIcon icon={props.icon} size={props.size ?? 14} color="var(--module-notes)" />;
}
