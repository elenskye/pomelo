// A site's icon (icons.rs, fetched once by the real app), else the host's first letter on a plain square.
import { hostOf, iconKey } from "./api";

export function SiteIcon(props: { url: string; icons: Record<string, string>; size?: number }) {
  const size = props.size ?? 16;
  const host = hostOf(props.url);
  const src = props.icons[iconKey(host)];
  if (src) return <img src={src} alt="" width={size} height={size} draggable={false} className="shrink-0 rounded-[3px] object-contain" />;
  const letter = (host.replace(/^www\./, "")[0] ?? "?").toUpperCase();
  return (
    <span aria-hidden className="grid shrink-0 place-items-center rounded-[4px] bg-muted font-bold text-muted-foreground" style={{ width: size, height: size, fontSize: Math.round(size * 0.62) }}>
      {letter}
    </span>
  );
}
