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
  server: "server",
  settings: "settings",
  terminal: "terminal",
  folder: "folder",
  activity: "activity",
  zap: "zap",
  sparkles: "sparkles",
  "chevron-left": "chevron-left",
  "chevron-right": "chevron-right",
  "chevron-down": "chevron-down",
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
