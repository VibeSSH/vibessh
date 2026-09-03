import { memo, type MouseEvent, type ReactNode } from "react";
import "./Markdown.css";

/**
 * The slice of markdown the guide is written in, rendered as React.
 *
 * Not a markdown library, and not `dangerouslySetInnerHTML`. The corpus is
 * a dozen files in this repository, written to this subset on purpose, so a
 * renderer that handles exactly that subset is smaller than the dependency
 * it replaces and cannot inject markup - every node below is a real element
 * with escaped text inside it.
 *
 * Vibe AI's answers render through here too, and that second caller is why
 * links and images are guarded: the guide's markdown is written in this
 * repository, a model's is not. Nothing here trusts the source it is given.
 *
 * What it renders: headings, paragraphs, bullet and numbered lists, fenced
 * code, tables, block quotes as call-outs, images, links, and inline
 * `code`, **bold** and _italic_. Anything else is shown as the literal text
 * it is, which is the right failure: a guide with a stray construct reads
 * slightly wrong rather than disappearing.
 */

/** Splits on the inline markers, keeping the delimiters so they can be
 * turned into elements. Order matters: code first, so `**` inside a code
 * span stays literal. */
function renderInline(text: string, keyPrefix: string): ReactNode[] {
  const nodes: ReactNode[] = [];
  const pattern = /(`[^`]+`|\*\*[^*]+\*\*|_[^_]+_|\[[^\]]+\]\([^)]+\))/g;
  let last = 0;
  let match: RegExpExecArray | null;
  let index = 0;

  while ((match = pattern.exec(text)) !== null) {
    if (match.index > last) nodes.push(text.slice(last, match.index));
    const token = match[0];
    const key = `${keyPrefix}-${index++}`;

    if (token.startsWith("`")) {
      nodes.push(<code key={key}>{token.slice(1, -1)}</code>);
    } else if (token.startsWith("**")) {
      nodes.push(<strong key={key}>{token.slice(2, -2)}</strong>);
    } else if (token.startsWith("_")) {
      nodes.push(<em key={key}>{token.slice(1, -1)}</em>);
    } else {
      const label = token.slice(1, token.indexOf("]"));
      const href = token.slice(token.indexOf("(") + 1, -1);
      // Guide links are internal by convention; anything else would need a
      // shell opener, which the guide has no reason to reach for.
      nodes.push(
        <a key={key} href={href}>
          {label}
        </a>,
      );
    }
    last = match.index + token.length;
  }
  if (last < text.length) nodes.push(text.slice(last));
  return nodes;
}

const BULLET = /^([-*])\s+/;
const NUMBERED = /^\d+[.)]\s+/;

function indentOf(line: string): number {
  return line.length - line.trimStart().length;
}

function listMarker(line: string): "ul" | "ol" | null {
  const trimmed = line.trim();
  if (BULLET.test(trimmed)) return "ul";
  if (NUMBERED.test(trimmed)) return "ol";
  return null;
}

/**
 * One list and everything indented under it, returned with the line to carry
 * on from.
 *
 * Indentation is the whole point of doing this recursively. A flat reader
 * ends the list at the first indented bullet, so a numbered list with
 * sub-points renders as three lists and the numbering restarts at 1 after
 * every one of them - which is exactly the shape the assistant answers in.
 */
function parseList(lines: string[], start: number, keyPrefix: string): [ReactNode, number] {
  const baseIndent = indentOf(lines[start]);
  const type = listMarker(lines[start]) ?? "ul";
  const items: { text: string; child: ReactNode | null }[] = [];
  let index = start;

  while (index < lines.length) {
    const line = lines[index];

    if (line.trim() === "") {
      // A blank line between items does not end the list. Markdown calls
      // this a loose list and it is how a model writes one, so stopping
      // here restarted the numbering at 1 on every single step - three
      // items rendering as "1. 1. 1.".
      let ahead = index + 1;
      while (ahead < lines.length && lines[ahead].trim() === "") ahead += 1;
      const continues = ahead < lines.length && indentOf(lines[ahead]) >= baseIndent && listMarker(lines[ahead]) !== null;
      if (!continues) break;
      index = ahead;
      continue;
    }

    const indent = indentOf(line);
    if (indent < baseIndent) break;

    if (indent > baseIndent) {
      // Deeper than this list, so it belongs to the item just read.
      if (listMarker(line) === null || items.length === 0) break;
      const [child, next] = parseList(lines, index, `${keyPrefix}-${items.length - 1}n`);
      items[items.length - 1].child = child;
      index = next;
      continue;
    }

    // A marker change at this level starts a different list, not this one.
    if (listMarker(line) !== type) break;
    items.push({ text: line.trim().replace(type === "ul" ? BULLET : NUMBERED, ""), child: null });
    index += 1;
  }

  const List = type;
  return [
    <List key={keyPrefix} className="guide-md-list">
      {items.map((item, i) => (
        <li key={i}>
          {renderInline(item.text, `${keyPrefix}-${i}`)}
          {item.child}
        </li>
      ))}
    </List>,
    index,
  ];
}

/** One `| a | b |` row, split on unescaped pipes. */
function tableCells(line: string): string[] {
  return line
    .trim()
    .replace(/^\|/, "")
    .replace(/\|$/, "")
    .split("|")
    .map((cell) => cell.trim());
}

function isTableDivider(line: string): boolean {
  return /^\|?[\s:-]+\|[\s|:-]*$/.test(line.trim()) && line.includes("-");
}

interface MarkdownProps {
  source: string;
  /** Resolves an image path in the markdown to something the app can load.
   * Screenshots live beside the documents, and how they are served is the
   * caller's problem, not the renderer's. Without one, no image is loaded:
   * the only source of URLs left would be whoever wrote the markdown, and
   * a model's answer is not a source this app fetches from. */
  resolveImage?: (src: string) => string | undefined;
  /** Called instead of following a link. A webview that follows an outside
   * URL has left the app, with no back button to return with, so any caller
   * whose markdown can carry arbitrary links must handle them itself. */
  onLinkClick?: (href: string) => void;
}

function MarkdownView({ source, resolveImage, onLinkClick }: MarkdownProps) {
  const lines = source.replace(/\r\n/g, "\n").split("\n");
  const blocks: ReactNode[] = [];
  let index = 0;
  let key = 0;

  while (index < lines.length) {
    const line = lines[index];
    const trimmed = line.trim();

    if (trimmed === "") {
      index += 1;
      continue;
    }

    // Fenced code. The closing fence is optional so an unterminated block
    // renders as code to the end rather than swallowing the page.
    if (trimmed.startsWith("```")) {
      const language = trimmed.slice(3).trim();
      const body: string[] = [];
      index += 1;
      while (index < lines.length && !lines[index].trim().startsWith("```")) {
        body.push(lines[index]);
        index += 1;
      }
      index += 1;
      blocks.push(
        <pre key={key++} className="guide-md-code" data-language={language || undefined}>
          <code>{body.join("\n")}</code>
        </pre>,
      );
      continue;
    }

    if (trimmed.startsWith("#")) {
      const level = trimmed.length - trimmed.replace(/^#+/, "").length;
      const text = trimmed.slice(level).trim();
      const Tag = (["h1", "h2", "h3", "h4", "h5", "h6"][Math.min(level, 6) - 1] ?? "h6") as "h1";
      blocks.push(
        <Tag key={key++} className={`guide-md-h guide-md-h${Math.min(level, 6)}`}>
          {renderInline(text, `h${key}`)}
        </Tag>,
      );
      index += 1;
      continue;
    }

    // An image on its own line is a figure, with its alt text as the caption
    // - a screenshot in a manual is worth naming.
    const image = /^!\[([^\]]*)\]\(([^)]+)\)$/.exec(trimmed);
    if (image) {
      const resolved = resolveImage ? resolveImage(image[2]) : undefined;
      index += 1;
      if (!resolved) {
        // Say what was there rather than dropping it silently - the same
        // choice the rest of this renderer makes for constructs it cannot
        // draw.
        if (image[1]) {
          blocks.push(
            <p key={key++} className="guide-md-p">
              {image[1]}
            </p>,
          );
        }
      } else {
        blocks.push(
          <figure key={key++} className="guide-md-figure">
            <img src={resolved} alt={image[1]} loading="lazy" />
            {image[1] && <figcaption>{image[1]}</figcaption>}
          </figure>,
        );
      }
      continue;
    }

    if (trimmed.startsWith(">")) {
      const body: string[] = [];
      while (index < lines.length && lines[index].trim().startsWith(">")) {
        body.push(lines[index].trim().replace(/^>\s?/, ""));
        index += 1;
      }
      blocks.push(
        <div key={key++} className="guide-md-callout">
          {renderInline(body.join(" "), `q${key}`)}
        </div>,
      );
      continue;
    }

    if (trimmed.startsWith("|") && index + 1 < lines.length && isTableDivider(lines[index + 1])) {
      const head = tableCells(lines[index]);
      index += 2;
      const rows: string[][] = [];
      while (index < lines.length && lines[index].trim().startsWith("|")) {
        rows.push(tableCells(lines[index]));
        index += 1;
      }
      blocks.push(
        <div key={key++} className="guide-md-table-scroll">
          <table className="guide-md-table">
            <thead>
              <tr>
                {head.map((cell, i) => (
                  <th key={i}>{renderInline(cell, `th${key}-${i}`)}</th>
                ))}
              </tr>
            </thead>
            <tbody>
              {rows.map((row, r) => (
                <tr key={r}>
                  {row.map((cell, c) => (
                    <td key={c}>{renderInline(cell, `td${key}-${r}-${c}`)}</td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>,
      );
      continue;
    }

    if (listMarker(line) !== null) {
      const [list, next] = parseList(lines, index, `li${key++}`);
      blocks.push(list);
      index = next;
      continue;
    }

    // A paragraph runs to the next blank line, so a sentence wrapped across
    // several lines in the source is still one paragraph.
    const paragraph: string[] = [];
    while (index < lines.length && lines[index].trim() !== "" && !lines[index].trim().startsWith("#") && !lines[index].trim().startsWith("```")) {
      paragraph.push(lines[index].trim());
      index += 1;
    }
    blocks.push(
      <p key={key++} className="guide-md-p">
        {renderInline(paragraph.join(" "), `p${key}`)}
      </p>,
    );
  }

  // Delegated rather than threaded through every inline call: one handler on
  // the container catches links wherever they turn up - prose, list items,
  // table cells.
  function handleClick(event: MouseEvent<HTMLDivElement>) {
    if (!onLinkClick) return;
    const anchor = (event.target as HTMLElement).closest("a");
    const href = anchor?.getAttribute("href");
    if (!href) return;
    event.preventDefault();
    onLinkClick(href);
  }

  return (
    <div className="guide-md" onClick={onLinkClick ? handleClick : undefined}>
      {blocks}
    </div>
  );
}

/** Memoised because Vibe AI re-renders the whole transcript on every
 * streamed token, and only the last message's text has changed. */
export const Markdown = memo(MarkdownView);
