import { parseDocument } from "yaml";

/**
 * One problem found in a YAML document, in editor coordinates.
 *
 * Deliberately not CodeMirror's `Diagnostic`: this shape is what the pure
 * function below returns and what its tests assert on, so the parsing can
 * be checked without an editor, a DOM or a mounted component.
 */
export interface YamlProblem {
  from: number;
  to: number;
  severity: "error" | "warning";
  message: string;
}

/**
 * Every problem in a YAML source, or an empty list.
 *
 * **Why a real parser rather than the syntax tree already in the editor.**
 * `@codemirror/lang-yaml` produces a Lezer tree with error nodes, so the
 * positions were available without a new dependency - but the message was
 * not. "Syntax error here" is worth very little in YAML, where nearly every
 * mistake is indentation and the useful sentence is the one naming what
 * went wrong: mappings not starting at the same column, an implicit key
 * spanning lines, a tab where spaces are required. That sentence is the
 * whole point of linting a config file, so it earns the dependency.
 *
 * **Why it never throws.** A linter that can fail takes the editor with it,
 * and the editor is how somebody fixes the file. `parseDocument` collects
 * errors rather than throwing, and anything it did not anticipate is
 * reported as a single problem instead of propagating.
 */
export function yamlProblems(source: string): YamlProblem[] {
  // Nothing to say about a file with nothing in it, and parsing it would
  // report a document with no contents as a problem.
  if (source.trim() === "") return [];

  try {
    const document = parseDocument(source, {
      // Duplicate keys are the mistake this catches that a human reading
      // the file will not: the second silently wins, so a setting appears
      // to be set to something it is not.
      uniqueKeys: true,
    });

    const problems: YamlProblem[] = [];
    for (const error of document.errors) {
      problems.push({ from: error.pos[0], to: Math.max(error.pos[1], error.pos[0] + 1), severity: "error", message: error.message });
    }
    for (const warning of document.warnings) {
      problems.push({ from: warning.pos[0], to: Math.max(warning.pos[1], warning.pos[0] + 1), severity: "warning", message: warning.message });
    }
    return problems;
  } catch (err) {
    // The parser is not supposed to reach here; if it does, say so at the
    // start of the file rather than leaving the editor with no linter.
    return [{ from: 0, to: 1, severity: "error", message: err instanceof Error ? err.message : "this file could not be parsed as YAML" }];
  }
}
