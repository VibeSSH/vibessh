import { describe, expect, it } from "vitest";
import { blockingProblems } from "./fileProblems";
import { yamlProblems } from "./yamlLint";

describe("blockingProblems", () => {
  it("blocks a save on YAML that does not parse", () => {
    const problems = blockingProblems("spigot.yml", "settings:\n  a: 1\n b: 2\n");
    expect(problems.length).toBeGreaterThan(0);
  });

  it("reports the line the gutter shows, not a document offset", () => {
    const [first] = blockingProblems("spigot.yml", "settings:\n  a: 1\n b: 2\n");
    expect(first.line).toBe(3);
  });

  it("lets valid YAML through", () => {
    expect(blockingProblems("spigot.yml", "settings:\n  a: 1\n  b: 2\n")).toEqual([]);
  });

  // A warning is the parser saying something is unusual. Refusing to save
  // over "unusual" would be the editor overruling the person editing.
  it("does not block on a warning", () => {
    // An unresolved tag: the parser warns and carries on, and the document
    // is still a document.
    const source = "key: !!unknownTag value\n";
    expect(yamlProblems(source).some((problem) => problem.severity === "warning")).toBe(true);
    expect(blockingProblems("config.yaml", source)).toEqual([]);
  });

  // A duplicate key is the one this exists for: the second silently wins, so
  // a setting reads as one value and is another, and nothing downstream ever
  // says so.
  it("blocks on a duplicate key", () => {
    expect(blockingProblems("spigot.yml", "a: 1\na: 2\n").length).toBe(1);
  });

  it("checks .yaml as well as .yml, whatever the case", () => {
    expect(blockingProblems("Config.YAML", "a:\n b:\nc: [\n").length).toBeGreaterThan(0);
  });

  // Everything else has no linter, so nothing can block it - a `.properties`
  // file has no syntax to be wrong about in the first place.
  it("never blocks a file it has no linter for", () => {
    expect(blockingProblems("server.properties", "a=1\n b\nc:::\n")).toEqual([]);
    expect(blockingProblems("notes.txt", "settings:\n  a: 1\n b: 2\n")).toEqual([]);
  });

  it("says nothing about an empty file", () => {
    expect(blockingProblems("spigot.yml", "")).toEqual([]);
  });
});
