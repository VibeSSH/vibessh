import { useEffect, useState } from "react";
import { yamlProblems, yamlProblemMessage } from "./yamlLint";

/**
 * A problem serious enough to stop a save, in terms the header can show.
 *
 * The editor's own linter reports in document offsets, which is what
 * CodeMirror needs and what nobody can read. A line number is what somebody
 * looking at a header note can act on.
 */
export interface BlockingProblem {
  line: number;
  message: string;
}

/** Which files are checked before saving. Only YAML has a linter, and only
 * a linter's errors can block - see `editorLanguage`'s own note for why YAML
 * is the format that earns one. */
function isLinted(fileName: string): boolean {
  const lower = fileName.toLowerCase();
  return lower.endsWith(".yml") || lower.endsWith(".yaml");
}

/** 1-based line of a document offset, counted the way the gutter counts. */
function lineOf(source: string, offset: number): number {
  let line = 1;
  for (let i = 0; i < offset && i < source.length; i += 1) {
    if (source[i] === "\n") line += 1;
  }
  return line;
}

/**
 * The errors that must be fixed before this file can be written.
 *
 * Errors only: a warning is the parser saying something is unusual, and
 * refusing to save over "unusual" would be the editor overruling the person
 * editing. An error means the document did not parse, which for a config
 * file means the service reading it will not start - and it will fail
 * minutes later, somewhere else, with the cause out of sight. That is the
 * one case where refusing is more helpful than obeying.
 */
export function blockingProblems(fileName: string, source: string, t: (key: string) => string): BlockingProblem[] {
  if (!isLinted(fileName)) return [];
  return yamlProblems(source)
    .filter((problem) => problem.severity === "error")
    .map((problem) => ({ line: lineOf(source, problem.from), message: yamlProblemMessage(problem, t) }));
}

/**
 * The same, recomputed a beat after typing stops.
 *
 * Parsing is not free on a large file and typing produces a state update per
 * keystroke, so this waits the way CodeMirror's own linter does rather than
 * parsing the document on every character. The delay is why a save is not
 * blocked mid-word: it is blocked once the file has settled into something
 * that does not parse.
 */
export function useBlockingProblems(fileName: string, source: string, t: (key: string) => string): BlockingProblem[] {
  const [problems, setProblems] = useState<BlockingProblem[]>([]);

  useEffect(() => {
    const timer = window.setTimeout(() => setProblems(blockingProblems(fileName, source, t)), 300);
    return () => window.clearTimeout(timer);
  }, [fileName, source, t]);

  return problems;
}
