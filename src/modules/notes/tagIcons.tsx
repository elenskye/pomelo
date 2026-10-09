// Tag icons: one of Pomelo's line icons (stored as "i:<key>") or any emoji the user types; no icon = #.
import {
  Bookmark, BookOpen, Brain, Briefcase, CalendarDays, Camera, Cat, Citrus, Clapperboard, Cloud, Code, Coffee, Dumbbell, Flame, Flower2,
  Gamepad2, Gift, Globe, GraduationCap, Hash, Heart, HeartPulse, Hourglass, House, Leaf, Library, Lightbulb, MapPin, MessageCircle, Moon,
  Music, NotebookPen, Palette, PenLine, Plane, Puzzle, Quote, Smile, Sparkles, Sprout, Star, Sun, Tag, Target, User, Users, Utensils, Wallet,
  Wrench, type LucideIcon,
} from "lucide-react";

export const TAG_ICONS: [string, LucideIcon][] = [
  ["book-open", BookOpen], ["library", Library], ["notebook", NotebookPen], ["pen", PenLine], ["lightbulb", Lightbulb], ["brain", Brain],
  ["message", MessageCircle], ["quote", Quote], ["bookmark", Bookmark], ["tag", Tag], ["target", Target], ["star", Star],
  ["sparkles", Sparkles], ["flame", Flame], ["heart", Heart], ["citrus", Citrus], ["sprout", Sprout], ["flower", Flower2],
  ["leaf", Leaf], ["moon", Moon], ["sun", Sun], ["cloud", Cloud], ["globe", Globe], ["plane", Plane],
  ["pin", MapPin], ["house", House], ["briefcase", Briefcase], ["wallet", Wallet], ["graduation", GraduationCap], ["wrench", Wrench],
  ["code", Code], ["film", Clapperboard], ["music", Music], ["palette", Palette], ["game", Gamepad2], ["camera", Camera],
  ["coffee", Coffee], ["food", Utensils], ["dumbbell", Dumbbell], ["health", HeartPulse], ["smile", Smile], ["user", User],
  ["users", Users], ["cat", Cat], ["gift", Gift], ["calendar", CalendarDays], ["hourglass", Hourglass], ["puzzle", Puzzle],
];
const BY_KEY = new Map(TAG_ICONS);

/** A tag's icon in a 16px slot: line icon (module color), emoji, or a muted #. */
export function TagIcon(props: { icon: string | null; size?: number }) {
  const size = props.size ?? 14;
  const Line = props.icon?.startsWith("i:") ? BY_KEY.get(props.icon.slice(2)) : undefined;
  if (Line) return <Line size={size} strokeWidth={1.75} style={{ color: "var(--module-notes)" }} />;
  if (props.icon && !props.icon.startsWith("i:")) return <span className="text-[13px] leading-none">{props.icon}</span>;
  return <Hash size={size - 1} strokeWidth={1.75} className="text-muted-foreground" />;
}
