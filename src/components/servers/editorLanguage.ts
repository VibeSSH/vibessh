import type { Extension } from "@codemirror/state";
import { StreamLanguage } from "@codemirror/language";
import { json } from "@codemirror/lang-json";
import { yaml } from "@codemirror/lang-yaml";
import { linter, lintGutter } from "@codemirror/lint";
import { yamlProblems, yamlProblemMessage } from "./yamlLint";
import { javascript } from "@codemirror/lang-javascript";
import { python } from "@codemirror/lang-python";
import { markdown } from "@codemirror/lang-markdown";
import { css } from "@codemirror/lang-css";
import { html } from "@codemirror/lang-html";
import { xml } from "@codemirror/lang-xml";
import { sql } from "@codemirror/lang-sql";
import { shell } from "@codemirror/legacy-modes/mode/shell";
import { propertiesLanguage } from "./propertiesLanguage";
import { nginx } from "@codemirror/legacy-modes/mode/nginx";
import { dockerFile } from "@codemirror/legacy-modes/mode/dockerfile";
import { toml } from "@codemirror/legacy-modes/mode/toml";

/**
 * Marks YAML syntax problems as you type.
 *
 * Only YAML gets a linter, and that is a judgement about consequence rather
 * than about effort. A broken `server.properties` line is ignored by the
 * server; a broken `spigot.yml` stops it booting, and the failure surfaces
 * minutes later as "the Application will not start" with the cause nowhere
 * in sight. Catching it in the editor is the difference between a red
 * squiggle and a diagnosis.
 *
 * `parseDocument` is not cheap on a large file, but CodeMirror already runs
 * a linter on a delay after typing stops rather than on every keystroke, so
 * this costs one parse per pause.
 */
const yamlLinter = (t: (key: string) => string) =>
  linter((view) =>
    yamlProblems(view.state.doc.toString()).map((problem) => ({
      from: Math.min(problem.from, view.state.doc.length),
      // Clamped: a parser position past the end of the document - which a
      // truncated file can produce - makes CodeMirror throw, and a linter
      // that can take the editor down is worse than no linter.
      to: Math.min(problem.to, view.state.doc.length),
      severity: problem.severity,
      message: yamlProblemMessage(problem, t),
      source: "YAML",
    })),
  );

/**
 * Picks a CodeMirror language extension from a remote file's name - by
 * extension first, then by a few filenames sysadmin work sees constantly
 * (Dockerfile, nginx site configs, systemd units) that don't carry one.
 * Returns an empty array (plain text, still gets the editor's theme/line
 * numbers) for anything unrecognized rather than guessing.
 *
 * Takes the translator because the YAML linter's messages are read by a
 * person, not by the editor - see `yamlProblemMessage`.
 */
export function languageExtensionFor(fileName: string, t: (key: string) => string): Extension[] {
  const lower = fileName.toLowerCase();
  const base = lower.split("/").pop() ?? lower;
  const ext = base.includes(".") ? base.slice(base.lastIndexOf(".") + 1) : "";

  switch (ext) {
    case "json":
      return [json()];
    case "yml":
    case "yaml":
      // `lintGutter` puts a marker beside the line number: the
      // underline only exists where the text is, and a config file is
      // usually longer than the window.
      return [yaml(), yamlLinter(t), lintGutter()];
    case "js":
    case "mjs":
    case "cjs":
      return [javascript()];
    case "jsx":
      return [javascript({ jsx: true })];
    case "ts":
    case "mts":
      return [javascript({ typescript: true })];
    case "tsx":
      return [javascript({ jsx: true, typescript: true })];
    case "py":
      return [python()];
    case "md":
    case "markdown":
      return [markdown()];
    case "css":
      return [css()];
    case "html":
    case "htm":
      return [html()];
    case "xml":
      return [xml()];
    case "sql":
      return [sql()];
    case "toml":
      return [StreamLanguage.define(toml)];
    case "sh":
    case "bash":
    case "zsh":
    case "bashrc":
    case "zshrc":
    case "profile":
      return [StreamLanguage.define(shell)];
    case "ini":
    case "cfg":
    case "conf":
    case "env":
    case "properties":
      return [propertiesLanguage];
  }

  if (base === "dockerfile" || base.startsWith("dockerfile.")) {
    return [StreamLanguage.define(dockerFile)];
  }
  if (base.includes("nginx") || lower.endsWith("/sites-available") || lower.includes("nginx/")) {
    return [StreamLanguage.define(nginx)];
  }
  if (base.endsWith(".service") || base.endsWith(".socket") || base.endsWith(".timer") || base.endsWith(".unit")) {
    return [propertiesLanguage];
  }
  if (base === ".env" || base.startsWith(".env.")) {
    return [propertiesLanguage];
  }

  return [];
}
