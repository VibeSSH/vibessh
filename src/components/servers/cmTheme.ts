import { EditorView } from "@codemirror/view";
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { tags as t } from "@lezer/highlight";
import type { Extension } from "@codemirror/state";

/**
 * CodeMirror theme + syntax highlighting built from VibeSSH's own tokens,
 * following the same structure as Voltius's cmTheme.ts (voltius/src/
 * components/filetransfer/editor/cmTheme.ts) - a CodeMirror EditorView.theme
 * driven by UI surface colors, plus a HighlightStyle driven by terminal ANSI
 * colors so syntax highlighting reads consistently with the Terminal module
 * (TerminalView.tsx uses this same palette). Voltius re-derives this per
 * custom theme; we don't have custom themes, so it's just built once from
 * the fixed values already in globals.css/TerminalView.tsx.
 */
const TERMINAL_COLORS = {
  red: "#ef4444",
  green: "#22c55e",
  yellow: "#eab308",
  blue: "#3b82f6",
  magenta: "#a855f7",
  cyan: "#06b6d4",
  brightCyan: "#22d3ee",
};

export function vibesshEditorTheme(): Extension[] {
  const view = EditorView.theme(
    {
      "&": {
        color: "var(--text-primary)",
        backgroundColor: "var(--surface-bg)",
        fontSize: "13px",
        height: "100%",
      },
      ".cm-content": {
        fontFamily: "var(--font-mono)",
        caretColor: "var(--accent)",
      },
      ".cm-scroller": {
        fontFamily: "var(--font-mono)",
        lineHeight: "1.5",
      },
      "&.cm-focused": { outline: "none" },
      ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--accent)" },
      "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection": {
        backgroundColor: "color-mix(in srgb, var(--accent) 25%, transparent)",
      },
      ".cm-gutters": {
        backgroundColor: "var(--surface-0)",
        color: "var(--text-tertiary)",
        border: "none",
        borderRight: "1px solid var(--border)",
      },
      ".cm-activeLineGutter": { backgroundColor: "var(--surface-2)", color: "var(--text-secondary)" },
      ".cm-activeLine": { backgroundColor: "color-mix(in srgb, var(--surface-2) 55%, transparent)" },
      ".cm-lineNumbers .cm-gutterElement": { padding: "0 8px 0 5px" },
      ".cm-foldPlaceholder": {
        backgroundColor: "var(--surface-2)",
        border: "none",
        color: "var(--text-secondary)",
      },
      "&.cm-editor .cm-matchingBracket": {
        backgroundColor: "var(--surface-2)",
        outline: "1px solid var(--border)",
      },
      ".cm-selectionMatch": { backgroundColor: "var(--surface-2)" },
      ".cm-panels": { backgroundColor: "var(--surface-0)", color: "var(--text-primary)" },
      ".cm-searchMatch": {
        backgroundColor: `${TERMINAL_COLORS.yellow}33`,
        outline: `1px solid ${TERMINAL_COLORS.yellow}`,
      },
      ".cm-searchMatch.cm-searchMatch-selected": { backgroundColor: `${TERMINAL_COLORS.yellow}55` },
      ".cm-tooltip": {
        backgroundColor: "var(--surface-1)",
        border: "1px solid var(--border)",
        color: "var(--text-primary)",
      },
    },
    { dark: true },
  );

  const highlight = HighlightStyle.define([
    { tag: [t.comment, t.lineComment, t.blockComment], color: "var(--text-tertiary)", fontStyle: "italic" },
    { tag: [t.keyword, t.modifier, t.operatorKeyword], color: TERMINAL_COLORS.magenta },
    { tag: [t.string, t.special(t.string), t.regexp], color: TERMINAL_COLORS.green },
    { tag: [t.number, t.bool, t.atom], color: TERMINAL_COLORS.yellow },
    { tag: [t.function(t.variableName), t.function(t.propertyName)], color: TERMINAL_COLORS.blue },
    { tag: [t.variableName, t.propertyName], color: "var(--text-primary)" },
    { tag: [t.typeName, t.className, t.namespace], color: TERMINAL_COLORS.cyan },
    { tag: [t.propertyName, t.attributeName], color: TERMINAL_COLORS.cyan },
    { tag: [t.tagName, t.angleBracket], color: TERMINAL_COLORS.red },
    { tag: [t.operator, t.punctuation, t.separator, t.bracket], color: "var(--text-secondary)" },
    { tag: [t.definitionKeyword, t.controlKeyword], color: TERMINAL_COLORS.magenta },
    { tag: [t.constant(t.name), t.standard(t.name)], color: TERMINAL_COLORS.brightCyan },
    { tag: [t.heading], color: TERMINAL_COLORS.blue, fontWeight: "bold" },
    { tag: [t.link, t.url], color: TERMINAL_COLORS.cyan, textDecoration: "underline" },
    { tag: [t.invalid], color: TERMINAL_COLORS.red },
  ]);

  return [view, syntaxHighlighting(highlight)];
}
