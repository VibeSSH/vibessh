import type { ReactNode } from "react";
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
   * caller's problem, not the renderer's. */
  resolveImage?: (src: string) => string | undefined;
}

export function Markdown({ source, resolveImage }: MarkdownProps) {
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
      const resolved = resolveImage ? resolveImage(image[2]) : image[2];
      index += 1;
      if (resolved) {
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

    const bullet = /^([-*])\s+/.test(trimmed);
    const numbered = /^\d+[.)]\s+/.test(trimmed);
    if (bullet || numbered) {
      const items: string[] = [];
      while (index < lines.length) {
        const candidate = lines[index].trim();
        const isItem = bullet ? /^([-*])\s+/.test(candidate) : /^\d+[.)]\s+/.test(candidate);
        if (!isItem) break;
        items.push(candidate.replace(bullet ? /^([-*])\s+/ : /^\d+[.)]\s+/, ""));
        index += 1;
      }
      const List = bullet ? "ul" : "ol";
      blocks.push(
        <List key={key++} className="guide-md-list">
          {items.map((item, i) => (
            <li key={i}>{renderInline(item, `li${key}-${i}`)}</li>
          ))}
        </List>,
      );
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

  return <div className="guide-md">{blocks}</div>;
}
