import { RangeSetBuilder } from "@codemirror/state";
import { Decoration, EditorView, ViewPlugin, type DecorationSet, type ViewUpdate } from "@codemirror/view";
import { forEachDiagnostic } from "@codemirror/lint";

/**
 * Marks the whole line a problem is on, instead of the span the parser
 * pointed at.
 *
 * The parser's range is usually a couple of characters, and for an
 * indentation error those characters are whitespace. Tinting exactly that
 * put a small red rectangle in the middle of empty space - a mark that
 * looked like a rendering fault rather than an error, which is what it was
 * reported as.
 *
 * A line reads as deliberate: the row is tinted and carries a bar down its
 * left edge, the same way an error banner does elsewhere in the app. It is
 * also the truer statement. "Every key at this level must start in the same
 * column" is about the line, not about the two spaces the parser stopped on.
 *
 * The diagnostics come from the lint state rather than from a second parse,
 * so this cannot disagree with the underline or the gutter about what is
 * wrong.
 */
const errorLine = Decoration.line({ class: "cm-problemLine cm-problemLine-error" });
const warningLine = Decoration.line({ class: "cm-problemLine cm-problemLine-warning" });

function buildDecorations(view: EditorView): DecorationSet {
  // Keyed by line start, because two diagnostics on one line must not each
  // add a decoration - and `RangeSetBuilder` requires them in order anyway.
  const severities = new Map<number, "error" | "warning">();

  forEachDiagnostic(view.state, (diagnostic, from) => {
    // A diagnostic can point past the end of a shrinking document between
    // the edit and the next lint pass.
    if (from > view.state.doc.length) return;
    const start = view.state.doc.lineAt(from).from;
    // An error outranks a warning on the same line: it is the one that
    // stops the file being saved.
    if (diagnostic.severity === "error" || !severities.has(start)) {
      severities.set(start, diagnostic.severity === "error" ? "error" : "warning");
    }
  });

  const builder = new RangeSetBuilder<Decoration>();
  for (const start of [...severities.keys()].sort((a, b) => a - b)) {
    builder.add(start, start, severities.get(start) === "error" ? errorLine : warningLine);
  }
  return builder.finish();
}

export const problemLineHighlight = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;

    constructor(view: EditorView) {
      this.decorations = buildDecorations(view);
    }

    update(update: ViewUpdate) {
      // Lint results arrive in their own transaction after the document
      // settles, so watching `docChanged` alone would leave the highlight a
      // beat behind the underline it is supposed to accompany.
      if (update.docChanged || update.viewportChanged || update.transactions.length > 0) {
        this.decorations = buildDecorations(update.view);
      }
    }
  },
  { decorations: (plugin) => plugin.decorations },
);
