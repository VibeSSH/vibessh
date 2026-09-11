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
  /** The parser's own sentence, in English. A fallback, not what is shown:
   * see `yamlProblemMessage`. */
  message: string;
  /** The parser's error code (`BAD_INDENT`, `DUPLICATE_KEY`, ...) - the
   * stable thing to translate on, since the message wording is the library's
   * and can change between releases. */
  code: string;
}

/**
 * The parser's sentence without the excerpt it appends.
 *
 * `parseDocument` prettifies its errors: after the sentence comes a blank
 * line, the offending lines of the file and a caret under the column. That
 * is useful in a terminal and noise in a one-line banner, where the file is
 * already on screen underneath with the line marked.
 */
function firstSentence(message: string): string {
  return message.split("\n")[0].replace(/:\s*$/, "");
}

/**
 * A problem in the interface's language, falling back to the parser's.
 *
 * The messages are the `yaml` package's own and are written for somebody
 * who knows the YAML spec: "All mapping items must start at the same
 * column" is exactly right and is not what a person editing `spigot.yml`
 * needs to read. Translating on the error code rather than the text keeps
 * this working when the library rewords something, and an unknown code
 * falls through to the English sentence - worse to read, still true.
 */
export function yamlProblemMessage(problem: YamlProblem, t: (key: string) => string): string {
  const key = `yamlError.${problem.code}`;
  const translated = t(key);
  return translated === key || translated === "" ? problem.message : translated;
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
      problems.push({
        from: error.pos[0],
        to: Math.max(error.pos[1], error.pos[0] + 1),
        severity: "error",
        message: firstSentence(error.message),
        code: error.code,
      });
    }
    for (const warning of document.warnings) {
      problems.push({
        from: warning.pos[0],
        to: Math.max(warning.pos[1], warning.pos[0] + 1),
        severity: "warning",
        message: firstSentence(warning.message),
        code: warning.code,
      });
    }
    return problems;
  } catch (err) {
    // The parser is not supposed to reach here; if it does, say so at the
    // start of the file rather than leaving the editor with no linter.
    return [
      {
        from: 0,
        to: 1,
        severity: "error",
        message: err instanceof Error ? firstSentence(err.message) : "this file could not be parsed as YAML",
        // Not one of the parser's codes, so it has a translation of its own.
        code: "UNPARSEABLE",
      },
    ];
  }
}
