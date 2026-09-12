import { describe, expect, it } from "vitest";
import en from "@/i18n/locales/en.json";
import pl from "@/i18n/locales/pl.json";

/**
 * Every refusal the cloud backend can return has a sentence in both
 * languages.
 *
 * The backend is the source of truth and it is a different language in a
 * different directory, so nothing else connects the two: a new
 * `Detail::new("something_new", ...)` in Rust would have shipped happily and
 * shown the user its English text inside a Polish sentence. That is the bug
 * this file exists to stop coming back - it is what produced "Brak
 * uprawnień: invalid email or password" on the sign-in screen.
 *
 * Reading the Rust rather than keeping a copied list here is the point. A
 * list would be one more thing to update, and the one that gets forgotten.
 *
 * Read through Vite rather than `node:fs`, the same way the navigation and
 * icon-coverage tests do: this project's tsconfig has no node types, and
 * adding them for one test would be a strange price to pay.
 */
const backendSources = import.meta.glob("../../../../backend/src/**/*.rs", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

function backendErrorCodes(): string[] {
  const codes = new Set<string>();
  for (const source of Object.values(backendSources)) {
    for (const match of source.matchAll(/Detail::new\(\s*"([a-z0-9_]+)"/g)) {
      codes.add(match[1]);
    }
  }
  return [...codes].sort();
}

describe("cloud backend error codes", () => {
  const codes = backendErrorCodes();

  it("finds the backend's codes at all", () => {
    // Guards the guard: if the glob or the regex ever stops matching, every
    // other assertion in this file passes vacuously - an empty `it.each`
    // reports no failures at all.
    expect(Object.keys(backendSources).length).toBeGreaterThan(5);
    expect(codes.length).toBeGreaterThan(30);
    expect(codes).toContain("invalid_credentials");
  });

  it.each(codes)("%s has an English sentence", (code) => {
    expect((en.cloudErrors as Record<string, string>)[code]).toBeTruthy();
  });

  it.each(codes)("%s has a Polish sentence", (code) => {
    expect((pl.cloudErrors as Record<string, string>)[code]).toBeTruthy();
  });

  it("has the same slots in both languages", () => {
    const slots = (text: string) => [...text.matchAll(/\{\{(\w+)\}\}/g)].map((m) => m[1]).sort();
    for (const code of codes) {
      const english = (en.cloudErrors as Record<string, string>)[code];
      const polish = (pl.cloudErrors as Record<string, string>)[code];
      // A slot present in one language and missing in the other renders as
      // a sentence with a hole in it, only for the users of that language.
      expect({ code, slots: slots(polish) }).toEqual({ code, slots: slots(english) });
    }
  });

  it("has no translations for codes the backend no longer raises", () => {
    const known = new Set(codes);
    const stale = Object.keys(en.cloudErrors as Record<string, string>).filter((code) => !known.has(code));
    expect(stale).toEqual([]);
  });
});
