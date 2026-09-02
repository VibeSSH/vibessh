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
      // The search panel is the one part of the editor made of form
      // controls, and CodeMirror ships its own look for them: a light
      // linear-gradient on every button and a plain bordered box for every
      // field. On this theme that reads as somebody else's widget dropped
      // into the app, so the whole panel is restyled from VibeSSH's tokens.
      //
      // `background` rather than `backgroundColor` throughout, because the
      // default is a gradient - setting only the colour leaves the gradient
      // painted over it, which is what made the buttons white.
      ".cm-panel.cm-search": {
        background: "var(--surface-0)",
        borderBottom: "1px solid var(--border)",
        display: "flex",
        flexWrap: "wrap",
        alignItems: "center",
        gap: "6px",
        padding: "8px 10px",
        fontFamily: "inherit",
        fontSize: "12.5px",
      },
      // The panel's own layout is a run of inline elements separated by
      // literal spaces and `<br>`; the flex gap above replaces those, so the
      // margins CodeMirror sets on each control have to go or they compound.
      ".cm-panel.cm-search > *": { margin: "0" },
      // The panel puts the replace row on its own line with a literal
      // `<br>`, which a flex container would otherwise swallow. Zero-height
      // and full-width is the standard way to keep it breaking the line.
      ".cm-panel.cm-search br": { flexBasis: "100%", height: "0" },
      ".cm-panel.cm-search .cm-textfield": {
        background: "var(--surface-bg)",
        border: "1px solid var(--border)",
        borderRadius: "var(--radius-sm)",
        color: "var(--text-primary)",
        font: "inherit",
        padding: "5px 10px",
        minWidth: "180px",
      },
      ".cm-panel.cm-search .cm-textfield::placeholder": { color: "var(--text-tertiary)" },
      ".cm-panel.cm-search .cm-textfield:focus": {
        borderColor: "var(--accent)",
        outline: "none",
      },
      // Matched to `.btn-sm`: same 28px floor, radius, weight and ring, so a
      // button in here and a button in the header above it are the same
      // control seen twice.
      ".cm-panel.cm-search .cm-button": {
        background: "var(--surface-1)",
        backgroundImage: "none",
        border: "1px solid transparent",
        borderRadius: "var(--radius-sm)",
        boxShadow: "var(--t-ring)",
        color: "var(--text-primary)",
        cursor: "pointer",
        font: "inherit",
        fontWeight: "600",
        minHeight: "28px",
        padding: "5px 12px",
      },
      ".cm-panel.cm-search .cm-button:hover": { background: "var(--surface-2)" },
      ".cm-panel.cm-search .cm-button:active": { filter: "brightness(0.95)" },
      ".cm-panel.cm-search label": {
        alignItems: "center",
        color: "var(--text-secondary)",
        display: "inline-flex",
        gap: "5px",
        whiteSpace: "nowrap",
      },
      ".cm-panel.cm-search label input": { accentColor: "var(--accent)", margin: "0" },
      // Pushed to the far end and drawn as a ghost control: it closes the
      // panel, which is the one thing in here nobody is aiming for.
      ".cm-panel.cm-search [name=close]": {
        background: "none",
        border: "none",
        borderRadius: "var(--radius-sm)",
        color: "var(--text-tertiary)",
        cursor: "pointer",
        fontSize: "18px",
        lineHeight: "1",
        marginLeft: "auto",
        padding: "2px 8px",
        position: "static",
      },
      ".cm-panel.cm-search [name=close]:hover": {
        background: "color-mix(in srgb, #ffffff 6%, transparent)",
        color: "var(--text-primary)",
      },
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
