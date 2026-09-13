import { describe, expect, it } from "vitest";
import en from "@/i18n/locales/en.json";
import pl from "@/i18n/locales/pl.json";

/**
 * Every application type reads in the language of the screen around it.
 *
 * A blueprint's name and description are authored in Rust, in English, and
 * cross the bridge as plain strings. Until `useBlueprintText` they went
 * straight onto the screen, so the grid somebody picks their application
 * type from - and the paragraph explaining what they are about to install -
 * was English in the middle of a Polish page.
 *
 * The translations fall back to the Rust string when a key is missing, which
 * is the right behaviour at runtime and exactly why this file has to exist:
 * a blueprint added without a Polish description would not fail, would not
 * warn, and would quietly be English again. Reading the Rust rather than
 * keeping a list here is the same choice `cloudErrorParity.test.ts` makes -
 * a list is one more thing to update, and the one that gets forgotten.
 */
const blueprintSources = import.meta.glob("../../../../src-tauri/src/blueprints/*.rs", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

/** Every blueprint id the app actually ships, read from its definition. */
function blueprintIds(): string[] {
  const ids = new Set<string>();
  for (const [path, source] of Object.entries(blueprintSources)) {
    if (path.endsWith("/mod.rs")) continue;
    const match = source.match(/id:\s*"([a-z0-9-]+)"\.to_string\(\)/);
    if (match) ids.add(match[1]);
  }
  return [...ids].sort();
}

/**
 * Typed on the one section this file reads, not on a whole locale: Polish
 * carries plural forms (`_few`, `_many`) that English has no key for, so
 * `typeof pl` and `typeof en` are two unrelated types and a helper written
 * against either one cannot be handed the other.
 */
const texts = (locale: { blueprints: unknown }) => locale.blueprints as Record<string, { name?: string; description?: string }>;

describe("blueprint text", () => {
  it("finds the blueprint definitions at all", () => {
    // Guards the guard: a moved directory would turn every assertion below
    // into a loop over nothing, which passes.
    const ids = blueprintIds();
    expect(ids.length).toBeGreaterThanOrEqual(14);
    expect(ids).toContain("postgres");
    expect(ids).toContain("mariadb");
  });

  for (const id of blueprintIds()) {
    it(`${id} has a description in both languages`, () => {
      expect(texts(pl)[id]?.description, `${id} has no Polish description`).toBeTruthy();
      expect(texts(en)[id]?.description, `${id} has no English description`).toBeTruthy();
    });
  }

  /// Product names stay as they are - "PostgreSQL" is not translated into
  /// anything - so a name entry is optional. Where one language gives it a
  /// name, though, the other has to as well, or the two screens disagree
  /// about what the same thing is called.
  it("translates a name in both languages or in neither", () => {
    for (const id of blueprintIds()) {
      const inPolish = Boolean(texts(pl)[id]?.name);
      const inEnglish = Boolean(texts(en)[id]?.name);
      expect(inPolish, `${id}: named in one language but not the other`).toBe(inEnglish);
    }
  });

  /// The two settings a PostgreSQL Application cannot start correctly
  /// without. The blueprint cannot apply them itself - see the Rust doc
  /// comment - so this text is the only thing standing between somebody and
  /// a database that works until the container is recreated.
  it("tells a PostgreSQL reader about both required settings, in both languages", () => {
    for (const locale of [pl, en]) {
      const description = texts(locale).postgres?.description ?? "";
      expect(description).toContain("POSTGRES_PASSWORD");
      expect(description).toContain("PGDATA=.");
      // And not `PGDATA=./pgdata`: the image's entrypoint has already
      // dropped to the `postgres` user by the time it creates the data
      // directory, so it cannot make a subdirectory in a working directory
      // that account does not own. Checked against a real container.
      expect(description).not.toContain("PGDATA=./");
    }
  });
});
