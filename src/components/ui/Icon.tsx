// The default @iconify/react entry ships an async, network-capable <Icon>
// (useState-backed, for icons not yet registered) that we never need, since
// every icon we use is registered synchronously below before anything
// renders - and hitting that async code path under Vite's dependency
// pre-bundling reliably threw "Cannot read properties of null (reading
// 'useState')" here. `/offline` is the sync-only build made for exactly
// this case (a bundled collection, nothing fetched at runtime).
import { addCollection, Icon as IconifyIcon } from "@iconify/react/offline";
import lucideSubset from "@/assets/lucide-subset.json";

let registered = false;
function ensureRegistered() {
  if (registered) return;
  registered = true;
  addCollection(lucideSubset);
}
ensureRegistered();

/**
 * Real lucide icons (same set Voltius uses via @iconify/react +
 * @iconify-json/lucide), not hand-approximated SVG paths - registered from a
 * small bundled subset (see scripts/generate-lucide-subset.mjs) so nothing
 * is ever fetched over the network. `name` keeps the same values every call
 * site already used before this switched to iconify - most map straight to
 * a lucide name, a couple (edit -> square-pen, trash -> trash-2) don't,
 * which is exactly what this map is for.
 */
const NAME_TO_LUCIDE: Record<string, string> = {
  "layout-grid": "layout-grid",
  "book-open": "book-open",
  // lucide renamed this one; the app's own name for it stays put.
  "help-circle": "circle-question-mark",
  server: "server",
  settings: "settings",
  terminal: "terminal",
  folder: "folder",
  activity: "activity",
  zap: "zap",
  sparkles: "sparkles",
  send: "send",
  "message-square": "message-square",
  "chevron-left": "chevron-left",
  "chevron-right": "chevron-right",
  "chevron-down": "chevron-down",
  "chevrons-left": "chevrons-left",
  "chevrons-right": "chevrons-right",
  plug: "plug",
  x: "x",
  copy: "copy",
  check: "check",
  key: "key",
  edit: "square-pen",
  trash: "trash-2",
  file: "file",
  play: "play",
  square: "square",
  power: "power",
  upload: "upload",
  download: "download",
  "refresh-cw": "refresh-cw",
  plus: "plus",
  "more-vertical": "ellipsis-vertical",
  search: "search",
  pin: "pin",
  wifi: "wifi",
  "wifi-off": "wifi-off",
  minus: "minus",
  bell: "bell",
  user: "user-round",
  users: "users",
  box: "box",
  // `eye`/`eye-off` were already used by Rail's host-reveal tooltip (and
  // are already in the bundled subset - see generate-lucide-subset.mjs) but
  // never actually mapped here, so that toggle has been silently rendering
  // nothing this whole time - fixed here rather than repeating the same
  // no-op icon in the Databases tab's own password-reveal toggle, which
  // reuses this exact pattern.
  eye: "eye",
  "eye-off": "eye-off",
  database: "database",
  move: "move",
  archive: "archive",
  history: "clock",
  lock: "lock",
  "alert-triangle": "triangle-alert",
  "list-checks": "list-checks",
  shield: "shield",
  cloud: "cloud",
  "arrow-left-right": "arrow-left-right",
};

interface IconProps {
  name: string;
  size?: number;
  className?: string;
}

export function Icon({ name, size = 18, className }: IconProps) {
  const lucideName = NAME_TO_LUCIDE[name];
  if (!lucideName) return null;
  return <IconifyIcon icon={`lucide:${lucideName}`} width={size} height={size} className={className} aria-hidden="true" />;
}
