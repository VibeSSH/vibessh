/**
 * The in-app guide, loaded from the same markdown the assistant reads.
 *
 * One corpus, two readers. `shared/guide/*.md` is embedded into the binary by
 * `ai::knowledge` for Vibe AI and imported here for the Guide page, so an
 * answer the assistant gives and a page the user opens cannot describe the
 * app differently - there is only one description. Adding a topic is adding
 * one file; nothing else has a list to update.
 *
 * The files live in `docs/` rather than under `src/` precisely so the Rust
 * side can `include_str!` them without reaching into the frontend's tree.
 */

/** One topic, as the sidebar and the reader need it. */
export interface GuideDoc {
  /** Stable identifier, used by `GuideLink` and the `?topic=` parameter. It
   * is the file name, so a link that works cannot silently stop working. */
  id: string;
  title: string;
  /** Which group it appears under in the sidebar. */
  section: string;
  /** Where in the app this topic actually is, for the "open it" button.
   * Absent for topics that describe something with no page of its own. */
  route?: string;
  /** Sort order within a section. Files without one sort last, by title. */
  order: number;
  /** Language this file is written in. */
  language: string;
  /** The markdown, frontmatter removed. */
  body: string;
  /** One line for the sidebar and for search results. */
  summary: string;
}

interface Frontmatter {
  fields: Record<string, string>;
  body: string;
}

/**
 * Reads the leading `--- ... ---` block.
 *
 * Deliberately not a YAML parser: the fields here are flat `key: value`
 * pairs, and every one of them is written by us. A file with no frontmatter
 * is not an error - it just has no fields, and the caller decides what is
 * missing.
 */
export function parseFrontmatter(source: string): Frontmatter {
  const normalised = source.replace(/\r\n/g, "\n");
  if (!normalised.startsWith("---\n")) return { fields: {}, body: normalised };

  const end = normalised.indexOf("\n---", 3);
  if (end === -1) return { fields: {}, body: normalised };

  const fields: Record<string, string> = {};
  for (const line of normalised.slice(4, end).split("\n")) {
    const separator = line.indexOf(":");
    if (separator === -1) continue;
    fields[line.slice(0, separator).trim()] = line.slice(separator + 1).trim();
  }
  // Past the closing `---` and the newline that follows it. The blank line
  // conventionally left after the block is dropped too, so the body starts
  // at the first thing somebody wrote - only newlines, never indentation,
  // which would break the first line of a code block.
  const bodyStart = normalised.indexOf("\n", end + 1);
  return { fields, body: bodyStart === -1 ? "" : normalised.slice(bodyStart + 1).replace(/^\n+/, "") };
}

/**
 * The first paragraph, for the sidebar.
 *
 * Written as prose in the file rather than duplicated into a frontmatter
 * field, so the summary somebody reads in the list is the sentence that
 * actually opens the topic - it cannot go stale relative to the page,
 * because it is the page.
 */
function firstParagraph(body: string): string {
  for (const block of body.split("\n\n")) {
    const text = block.trim();
    if (text === "" || text.startsWith("#") || text.startsWith("!") || text.startsWith(">")) continue;
    return text.replace(/\s+/g, " ");
  }
  return "";
}

/** `ports.pl.md` -> `{ id: "ports", language: "pl" }`. */
export function parseFileName(path: string): { id: string; language: string } | null {
  const name = path.split("/").pop() ?? path;
  const match = /^(.+)\.([a-z]{2})\.md$/.exec(name);
  if (!match) return null;
  return { id: match[1], language: match[2] };
}

export function toGuideDoc(path: string, source: string): GuideDoc | null {
  const named = parseFileName(path);
  if (!named) return null;
  const { fields, body } = parseFrontmatter(source);
  return {
    id: named.id,
    language: named.language,
    title: fields.title ?? named.id,
    section: fields.section ?? "",
    route: fields.route || undefined,
    // Unordered files sort after ordered ones rather than jumping to the
    // front, which is what an absent field parsed as 0 would do.
    order: fields.order ? Number(fields.order) : Number.MAX_SAFE_INTEGER,
    body,
    summary: firstParagraph(body),
  };
}

const rawFiles = import.meta.glob("../../../../../shared/guide/*.md", { query: "?raw", import: "default", eager: true }) as Record<string, string>;

const allDocs: GuideDoc[] = Object.entries(rawFiles)
  .map(([path, source]) => toGuideDoc(path, source))
  .filter((doc): doc is GuideDoc => doc !== null);

/**
 * Every topic in one language, in reading order.
 *
 * Falls back to any other language for a topic that has not been written in
 * the requested one yet, rather than hiding it: a page in the wrong language
 * is worth more than a gap where a feature's documentation should be, and
 * the reader can see which it is from `doc.language`.
 */
export function guideDocs(language: string): GuideDoc[] {
  const wanted = language.split("-")[0];
  const byId = new Map<string, GuideDoc>();
  for (const doc of allDocs) {
    const existing = byId.get(doc.id);
    if (!existing || (existing.language !== wanted && doc.language === wanted)) {
      byId.set(doc.id, doc);
    }
  }
  return [...byId.values()].sort((a, b) => a.order - b.order || a.title.localeCompare(b.title));
}

export function guideDoc(id: string, language: string): GuideDoc | undefined {
  return guideDocs(language).find((doc) => doc.id === id);
}

/**
 * Topics matching a query, best first.
 *
 * Title matches outrank body matches, because somebody typing "port" is
 * looking for the Ports topic rather than for every page that mentions one.
 */
export function searchGuide(query: string, language: string): GuideDoc[] {
  const needle = query.trim().toLowerCase();
  if (needle === "") return guideDocs(language);
  return guideDocs(language)
    .map((doc) => {
      const title = doc.title.toLowerCase().includes(needle) ? 2 : 0;
      const body = doc.body.toLowerCase().includes(needle) ? 1 : 0;
      return { doc, score: title + body };
    })
    .filter((scored) => scored.score > 0)
    .sort((a, b) => b.score - a.score)
    .map((scored) => scored.doc);
}
