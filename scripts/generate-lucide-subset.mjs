// Regenerates apps/desktop/ui/src/assets/lucide-subset.json - a hand-picked slice of
// @iconify-json/lucide's full icon set (1800+ icons), bundled into the app
// so <Icon> never fetches icon data over the network at runtime (Voltius
// solves this at build time with a custom Vite plugin that auto-scans
// icon="lucide:x" usages; VibeSSH's icon set is small and static enough
// that a checked-in subset, hand-extended when a new icon is needed, is
// the simpler equivalent).
//
// Run after adding a new icon name to ICON_NAMES:
//   node scripts/generate-lucide-subset.mjs

import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const ICON_NAMES = [
  // The guide: its own place in the rail, and the "what is this?"
  // button that sits next to a feature.
  "book-open",
  "circle-question-mark",
  "layout-grid",
  "server",
  "settings",
  "terminal",
  "folder",
  "activity",
  "zap",
  "sparkles",
  "send",
  "message-square",
  "chevron-left",
  "chevron-right",
  "chevron-down",
  "chevron-up",
  "external-link",
  "chevrons-left",
  "chevrons-right",
  "plug",
  "x",
  "copy",
  // Cut and paste in the text-field and editor right-click menus.
  "scissors",
  "clipboard",
  "check",
  "key",
  "square-pen",
  "trash-2",
  "file",
  "play",
  "square",
  "power",
  "upload",
  "download",
  "refresh-cw",
  "plus",
  "ellipsis-vertical",
  "search",
  "pin",
  "wifi",
  "wifi-off",
  "minus",
  "bell",
  "user-round",
  "users",
  "folder-plus",
  "file-plus",
  "eye",
  "eye-off",
  "box",
  "database",
  "move",
  "archive",
  "clock",
  "lock",
  "triangle-alert",
  "list-checks",
  "shield",
  "cloud",
  "arrow-left-right",
];

const root = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
// Dependencies hoist to the workspace root; the subset belongs to the UI
// package that imports it. Step 1 of docs/repository-structure.md split
// those two apart.
const ui = path.join(root, "apps", "desktop", "ui");
const source = JSON.parse(readFileSync(path.join(root, "node_modules/@iconify-json/lucide/icons.json"), "utf8"));

const missing = ICON_NAMES.filter((name) => !source.icons[name]);
if (missing.length > 0) {
  console.error(`Not found in @iconify-json/lucide: ${missing.join(", ")}`);
  process.exit(1);
}

const subset = {
  prefix: "lucide",
  width: source.width,
  height: source.height,
  icons: Object.fromEntries(ICON_NAMES.map((name) => [name, source.icons[name]])),
};

const outPath = path.join(ui, "src/assets/lucide-subset.json");
writeFileSync(outPath, JSON.stringify(subset, null, 2) + "\n");
console.log(`Wrote ${ICON_NAMES.length} icons to ${path.relative(root, outPath)}`);
