import { describe, expect, it } from "vitest";

/**
 * Every application type has a page in the guide, in both languages.
 *
 * This file exists because the gap it checks for actually shipped. A
 * PostgreSQL blueprint went out with no `app-postgres` page and nothing
 * noticed: the guide loads whatever files are in `shared/guide`, so a
 * missing one is not an error, it is simply a topic that is not there. The
 * person it fails is the one who picks the new type in the wizard and looks
 * for the instructions.
 *
 * Both sides are read from the source rather than listed here - a list would
 * be a third place to update and the one that gets forgotten, which is the
 * same mistake one level up.
 */
const blueprintSources = import.meta.glob("../../../src-tauri/src/blueprints/*.rs", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

const guideFiles = import.meta.glob("../../../../../shared/guide/*.md", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

/** Ids of the application types the app actually offers. */
function blueprintIds(): string[] {
  const ids = new Set<string>();
  for (const [path, source] of Object.entries(blueprintSources)) {
    if (path.endsWith("/mod.rs")) continue;
    const match = source.match(/id:\s*"([a-z0-9-]+)"\.to_string\(\)/);
    if (match) ids.add(match[1]);
  }
  return [...ids].sort();
}

/** The guide file names present, as `app-postgres.pl` and the like. */
function guideTopics(): Set<string> {
  const topics = new Set<string>();
  for (const path of Object.keys(guideFiles)) {
    const name = path.split("/").pop() ?? "";
    topics.add(name.replace(/\.md$/, ""));
  }
  return topics;
}

describe("guide coverage", () => {
  it("finds both sides at all", () => {
    // Guards the guard: a moved directory would make every assertion below
    // loop over nothing, which passes.
    expect(blueprintIds().length).toBeGreaterThanOrEqual(15);
    expect(guideTopics().size).toBeGreaterThanOrEqual(30);
  });

  for (const id of blueprintIds()) {
    for (const language of ["pl", "en"]) {
      it(`${id} has a ${language} page`, () => {
        expect(guideTopics().has(`app-${id}.${language}`), `shared/guide/app-${id}.${language}.md is missing`).toBe(true);
      });
    }
  }

  /// The guide is written for somebody who has not done this before, so a
  /// page that is only a paragraph is not a page. The shortest real one runs
  /// to a few hundred words; this catches a stub, not a style.
  it("gives every application type more than a stub", () => {
    for (const id of blueprintIds()) {
      for (const language of ["pl", "en"]) {
        const path = Object.keys(guideFiles).find((candidate) => candidate.endsWith(`/app-${id}.${language}.md`));
        if (!path) continue; // Reported by the test above; not worth failing twice.
        expect(guideFiles[path].length, `app-${id}.${language}.md is too short to be instructions`).toBeGreaterThan(600);
      }
    }
  });
});
