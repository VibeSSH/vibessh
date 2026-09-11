import { describe, expect, it } from "vitest";
import { parseFrontmatter, parseFileName, toGuideDoc } from "./guideDocs";

const DOC = `---
id: ports
title: Porty
section: applications
route: /applications
order: 30
---

Port to deklaracja.

## Gdzie to jest

Aplikacja, zakładka Porty.
`;

describe("parseFrontmatter", () => {
  it("reads the fields and hands back the body without them", () => {
    const { fields, body } = parseFrontmatter(DOC);
    expect(fields.title).toBe("Porty");
    expect(fields.route).toBe("/applications");
    expect(body.startsWith("Port to deklaracja.")).toBe(true);
  });

  // A document without frontmatter is content, not an error.
  it("treats a file with no frontmatter as all body", () => {
    const { fields, body } = parseFrontmatter("# Title\n\ntext\n");
    expect(fields).toEqual({});
    expect(body).toBe("# Title\n\ntext\n");
  });

  it("survives an unterminated frontmatter block", () => {
    const { body } = parseFrontmatter("---\ntitle: x\n\nno closing fence\n");
    expect(body).toContain("no closing fence");
  });

  // Files written on Windows arrive with CRLF; the parser must not leave a
  // stray carriage return on every value.
  it("reads a file with Windows line endings", () => {
    const { fields } = parseFrontmatter("---\r\ntitle: Porty\r\n---\r\n\r\nbody\r\n");
    expect(fields.title).toBe("Porty");
  });
});

describe("parseFileName", () => {
  it("splits the topic from its language", () => {
    expect(parseFileName("../../../../../shared/guide/ports.pl.md")).toEqual({ id: "ports", language: "pl" });
    expect(parseFileName("application-files.en.md")).toEqual({ id: "application-files", language: "en" });
  });

  it("ignores a file that does not name its language", () => {
    expect(parseFileName("ports.md")).toBeNull();
  });
});

describe("toGuideDoc", () => {
  it("summarises with the first real paragraph", () => {
    const doc = toGuideDoc("ports.pl.md", DOC);
    expect(doc?.summary).toBe("Port to deklaracja.");
  });

  // Without this an unordered file would sort to the very front, ahead of
  // everything that took the trouble to say where it belongs.
  it("sorts a file with no order last", () => {
    const doc = toGuideDoc("x.en.md", "---\ntitle: X\n---\n\nbody\n");
    expect(doc?.order).toBe(Number.MAX_SAFE_INTEGER);
  });

  it("keeps the route only when there is one", () => {
    expect(toGuideDoc("ports.pl.md", DOC)?.route).toBe("/applications");
    expect(toGuideDoc("x.en.md", "---\ntitle: X\nroute:\n---\n\nbody\n")?.route).toBeUndefined();
  });
});
