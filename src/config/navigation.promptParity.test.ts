import { describe, expect, it } from "vitest";
// Read through Vite rather than through `node:fs`: this project's tsconfig
// covers `src` with the browser lib only, and `?raw` is already how the
// guide loads its own markdown.
import promptSource from "../../src-tauri/src/ai/prompt.rs?raw";
import en from "@/i18n/locales/en.json";
import { sidebarGroups } from "./navigation";

/**
 * The assistant's system prompt enumerates the app's screens so the model can
 * refuse to invent one. That list lives in Rust, the navigation lives here,
 * and nothing but this test connects them.
 *
 * It exists because the gap was not theoretical: asked how to check whether
 * two Nodes were connected, the assistant sent the user to "Tools > WireGuard
 * network", a screen this app has never had. It filled a hole in the prompt
 * with something plausible, which is the one thing a support answer must not
 * do.
 */
describe("the system prompt's list of screens", () => {
  const prompt = promptSource;
  // The prompt wraps long lines with a trailing backslash, so a name can be
  // split across two source lines.
  const unwrapped = prompt.replace(/\\r?\n\s*/g, "");

  // Scoped to the enumeration itself. Matching the whole file would pass on
  // any screen whose name happens to appear somewhere else in the prompt -
  // "Files", "Monitor" and "Databases" are all names of Application tabs too,
  // so a whole-file match proves nothing about the list.
  const marker = "These are all the sidebar entries there are:";
  const start = unwrapped.indexOf(marker);
  const list = start === -1 ? "" : unwrapped.slice(start + marker.length, unwrapped.indexOf(".", start + marker.length));

  const labels = sidebarGroups.flatMap((group) => group.items).map((item) => ({
    id: item.id,
    label: (en.nav as Record<string, string>)[item.labelKey.replace(/^nav\./, "")],
  }));

  it("still contains the enumeration this test is about", () => {
    expect(start, `"${marker}" is gone from the prompt`).toBeGreaterThan(-1);
    expect(list.length).toBeGreaterThan(0);
  });

  it.each(labels)("names $label", ({ label }) => {
    expect(label, "every navigation item must have an English label").toBeTruthy();
    expect(list).toContain(label);
  });
});
