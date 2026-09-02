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
      // The modifier toggles are bare `<input type="checkbox">` with no
      // wrapper to hang a box on, so unlike the app's own Checkbox they have
      // to be drawn on the input itself. `accent-color` alone was not
      // enough: it tints the checked state and leaves the unchecked box as
      // the platform's own light grey square, which is what still stood out
      // against the dark panel. Sized, rounded and coloured to match
      // Checkbox.css so the two read as one control.
      ".cm-panel.cm-search label input[type=checkbox]": {
        appearance: "none",
        background: "var(--surface-1)",
        border: "1.5px solid var(--border-hover)",
        borderRadius: "5px",
        cursor: "pointer",
        height: "16px",
        margin: "0",
        transition: "background-color 150ms ease, border-color 150ms ease",
        width: "16px",
      },
      ".cm-panel.cm-search label input[type=checkbox]:hover": { borderColor: "var(--accent)" },
      ".cm-panel.cm-search label input[type=checkbox]:checked": {
        background: "var(--accent)",
        borderColor: "var(--accent)",
        // The tick has to be drawn here rather than by an icon component,
        // and a data URI cannot read a custom property - this is
        // `--accent-contrast` written out.
        backgroundImage:
          "url(\"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 16 16'%3E%3Cpath fill='none' stroke='%2304141c' stroke-width='2.5' stroke-linecap='round' stroke-linejoin='round' d='m4 8.5 2.5 2.5L12 5.5'/%3E%3C/svg%3E\")",
        backgroundSize: "100% 100%",
      },
      ".cm-panel.cm-search label input[type=checkbox]:focus-visible": {
        outline: "2px solid var(--accent)",
        outlineOffset: "2px",
      },
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
      // Lint marks: the line, not the two characters the parser stopped on.
      //
      // The first attempt tinted the diagnostic's own range. For an
      // indentation error that range is whitespace, so it drew a small red
      // rectangle in the middle of empty space - reported, fairly, as
      // looking broken. The row is marked instead (see `problemLines`), with
      // a bar down its left edge the same way an error banner is marked
      // elsewhere in the app, and the range keeps only its underline.
      // No `currentColor` for the bar: `color` on the line element is
      // inherited by every unhighlighted run of text in it, which would
      // repaint the file's own words red.
      ".cm-problemLine-error": {
        backgroundColor: "color-mix(in srgb, #ef4444 9%, transparent)",
        boxShadow: "inset 2px 0 0 0 #ef4444",
      },
      ".cm-problemLine-warning": {
        backgroundColor: "color-mix(in srgb, #f59e0b 8%, transparent)",
        boxShadow: "inset 2px 0 0 0 #f59e0b",
      },
      ".cm-lintRange-error": {
        backgroundImage:
          "url(\"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='6' height='3'%3E%3Cpath fill='none' stroke='%23ef4444' stroke-width='1.2' d='m0 3 1.5-2 1.5 2 1.5-2 1.5 2'/%3E%3C/svg%3E\")",
        backgroundRepeat: "repeat-x",
        backgroundPosition: "left bottom",
        paddingBottom: "1px",
      },
      ".cm-lintRange-warning": {
        backgroundImage:
          "url(\"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='6' height='3'%3E%3Cpath fill='none' stroke='%23f59e0b' stroke-width='1.2' d='m0 3 1.5-2 1.5 2 1.5-2 1.5 2'/%3E%3C/svg%3E\")",
        backgroundRepeat: "repeat-x",
        backgroundPosition: "left bottom",
      },
      // The gutter's own marker is a filled disc as wide as the column, which
      // next to a fold arrow and a line number is one round shape too many.
      // A small dot, vertically centred, aligned with the bar on the line.
      ".cm-gutter-lint": { width: "12px" },
      ".cm-gutter-lint .cm-gutterElement": { padding: "0", display: "flex", alignItems: "center", justifyContent: "center" },
      ".cm-lint-marker": {
        backgroundImage: "none",
        borderRadius: "50%",
        content: "''",
        height: "6px",
        width: "6px",
      },
      ".cm-lint-marker-error": { backgroundColor: "#ef4444", content: "''" },
      ".cm-lint-marker-warning": { backgroundColor: "#f59e0b", content: "''" },
      ".cm-tooltip-lint": {
        background: "var(--surface-1)",
        border: "1px solid var(--border)",
        borderRadius: "var(--radius-sm)",
      },
      ".cm-diagnostic": {
        borderLeft: "3px solid transparent",
        fontFamily: "inherit",
        fontSize: "12.5px",
        padding: "6px 10px",
      },
      ".cm-diagnostic-error": { borderLeftColor: "#ef4444" },
      ".cm-diagnostic-warning": { borderLeftColor: "#f59e0b" },
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
