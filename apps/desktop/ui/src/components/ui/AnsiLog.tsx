import { forwardRef, useMemo, type CSSProperties, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { logLevelOf } from "@/components/applications/logLevel";
import { hasAnsi, parseAnsiLine, splitDockerTimestamp, type AnsiSegment, type AnsiStyle } from "./ansi";
import "./AnsiLog.css";

interface AnsiLogProps {
  lines: string[];
  /** Shown instead of the lines while there are none - loading, empty. */
  placeholder?: ReactNode;
  className?: string;
}

interface RenderedLine {
  time: string | null;
  timeTitle: string | null;
  segments: AnsiSegment[];
  /** Set only for a line the server did not colour itself. */
  level: "error" | "warn" | "debug" | null;
}

function styleOf(style: AnsiStyle): CSSProperties | undefined {
  if (!style.color && !style.bold && !style.dim && !style.italic && !style.underline) return undefined;
  return {
    color: style.color,
    fontWeight: style.bold ? 600 : undefined,
    opacity: style.dim ? 0.7 : undefined,
    fontStyle: style.italic ? "italic" : undefined,
    textDecoration: style.underline ? "underline" : undefined,
  };
}

/**
 * A block of log output with the colours the process wrote into it.
 *
 * Replaces a bare `<pre>` that printed every ANSI escape as text. Two things
 * beyond the colours, both matching the application console:
 *
 * - Docker's timestamp is kept, but quiet and short. A Minecraft server
 *   stamps its own lines too, and the 30-character RFC 3339 prefix in front
 *   of that was the loudest thing on every line. The full value is on hover.
 * - A line the server left uncoloured is shaded by its level, so the stack
 *   trace under an ERROR is not the same grey as ordinary output. A line the
 *   server did colour is left exactly as it was sent.
 */
export const AnsiLog = forwardRef<HTMLPreElement, AnsiLogProps>(function AnsiLog({ lines, placeholder, className }, ref) {
  const { i18n } = useTranslation();

  const rendered = useMemo<RenderedLine[]>(() => {
    const today = new Date().toDateString();
    const timeFormat = new Intl.DateTimeFormat(i18n.language, { hour: "2-digit", minute: "2-digit", second: "2-digit" });
    const dateTimeFormat = new Intl.DateTimeFormat(i18n.language, { day: "2-digit", month: "2-digit", hour: "2-digit", minute: "2-digit", second: "2-digit" });
    let carried: AnsiStyle = {};
    return lines.map((line) => {
      const { timestamp, raw, rest } = splitDockerTimestamp(line);
      const coloured = hasAnsi(rest) || Object.keys(carried).length > 0;
      const { segments, style } = parseAnsiLine(rest, carried);
      carried = style;
      const level = coloured ? null : logLevelOf(rest);
      return {
        time: timestamp ? (timestamp.toDateString() === today ? timeFormat : dateTimeFormat).format(timestamp) : null,
        timeTitle: raw,
        segments,
        level: level === "info" ? null : level,
      };
    });
  }, [lines, i18n.language]);

  return (
    <pre className={`ansi-log${className ? ` ${className}` : ""}`} ref={ref}>
      {lines.length === 0
        ? placeholder
        : rendered.map((line, index) => (
            <div key={index} className={`ansi-log-line${line.level ? ` ansi-log-${line.level}` : ""}`}>
              {line.time && (
                <span className="ansi-log-time" title={line.timeTitle ?? undefined}>
                  {/* The space is text, not margin, so a copied line keeps it. */}
                  {line.time}{" "}
                </span>
              )}
              {line.segments.map((segment, segmentIndex) => (
                <span key={segmentIndex} style={styleOf(segment.style)}>
                  {segment.text}
                </span>
              ))}
            </div>
          ))}
    </pre>
  );
});
