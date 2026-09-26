import { describe, expect, it } from "vitest";
// Read through Vite rather than `node:fs`, the same way the navigation/prompt
// parity test does - this project's tsconfig covers `src` with the browser
// lib only.
import iconSource from "./Icon.tsx?raw";
import subset from "@/assets/lucide-subset.json";

/**
 * Every icon the app asks for has to actually exist.
 *
 * `<Icon>` resolves a name through `NAME_TO_LUCIDE` and then looks the result
 * up in a bundled subset of lucide. Miss either step and it renders **nothing
 * at all** - no error, no fallback glyph, just an empty box where a button's
 * icon should be. That is not theoretical: `external-link` on the Database
 * Hosts page shipped as a blank button, and a sweep written to find it turned
 * up three more (`chevron-up`, `file-plus`, `folder-plus`).
 *
 * The failure is silent by nature, so it needs a test rather than an eye.
 */
describe("every icon name used in the app", () => {
  const mapBlock = iconSource.slice(iconSource.indexOf("const NAME_TO_LUCIDE"));
  const mapped = new Map<string, string>();
  for (const match of mapBlock.slice(0, mapBlock.indexOf("\n};")).matchAll(/^\s*(?:"([a-z0-9-]+)"|([a-z][a-zA-Z0-9]*))\s*:\s*"([a-z0-9-]+)"/gm)) {
    mapped.set(match[1] ?? match[2], match[3]);
  }

  const available = new Set(Object.keys((subset as { icons: Record<string, unknown> }).icons));

  // Every source file, read at build time. Tests are excluded: an
  // `aria-label="subject"` in one is not an icon.
  const sources = import.meta.glob("/src/**/*.{ts,tsx}", { query: "?raw", import: "default", eager: true }) as Record<string, string>;

  const used = new Map<string, string[]>();
  for (const [path, text] of Object.entries(sources)) {
    if (path.includes(".test.")) continue;
    for (const match of text.matchAll(/\b(?:icon|iconName)=["']([a-z][a-z0-9-]*)["']/g)) {
      used.set(match[1], [...(used.get(match[1]) ?? []), path]);
    }
    for (const match of text.matchAll(/<Icon\s+name=["']([a-z][a-z0-9-]*)["']/g)) {
      used.set(match[1], [...(used.get(match[1]) ?? []), path]);
    }
    // Menu items and similar objects: `{ label, icon: "scissors" }`. The two
    // forms above missed these, and cut and paste shipped without icons in
    // every right-click menu. Not in the shadcn primitives, where `icon:` is a
    // button size variant holding class names.
    if (path.includes("/shadcn/")) continue;
    for (const match of text.matchAll(/\bicon:\s*["']([a-z][a-z0-9-]*)["']/g)) {
      used.set(match[1], [...(used.get(match[1]) ?? []), path]);
    }
  }

  it("finds icon usages at all, so a broken scan cannot pass by finding nothing", () => {
    expect(used.size).toBeGreaterThan(30);
    expect(mapped.size).toBeGreaterThan(30);
  });

  it.each([...used.keys()].sort())("%s resolves to a lucide name that is bundled", (name) => {
    const lucide = mapped.get(name);
    expect(lucide, `"${name}" is missing from NAME_TO_LUCIDE - used in ${used.get(name)?.join(", ")}`).toBeDefined();
    expect(available.has(lucide as string), `"${name}" maps to "${lucide}", which is not in lucide-subset.json - add it to scripts/generate-lucide-subset.mjs and re-run it`).toBe(true);
  });
});
