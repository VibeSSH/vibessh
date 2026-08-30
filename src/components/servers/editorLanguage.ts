import type { Extension } from "@codemirror/state";
import { StreamLanguage } from "@codemirror/language";
import { json } from "@codemirror/lang-json";
import { yaml } from "@codemirror/lang-yaml";
import { javascript } from "@codemirror/lang-javascript";
import { python } from "@codemirror/lang-python";
import { markdown } from "@codemirror/lang-markdown";
import { css } from "@codemirror/lang-css";
import { html } from "@codemirror/lang-html";
import { xml } from "@codemirror/lang-xml";
import { sql } from "@codemirror/lang-sql";
import { shell } from "@codemirror/legacy-modes/mode/shell";
import { properties } from "@codemirror/legacy-modes/mode/properties";
import { nginx } from "@codemirror/legacy-modes/mode/nginx";
import { dockerFile } from "@codemirror/legacy-modes/mode/dockerfile";
import { toml } from "@codemirror/legacy-modes/mode/toml";

/**
 * Picks a CodeMirror language extension from a remote file's name - by
 * extension first, then by a few filenames sysadmin work sees constantly
 * (Dockerfile, nginx site configs, systemd units) that don't carry one.
 * Returns an empty array (plain text, still gets the editor's theme/line
 * numbers) for anything unrecognized rather than guessing.
 */
export function languageExtensionFor(fileName: string): Extension[] {
  const lower = fileName.toLowerCase();
  const base = lower.split("/").pop() ?? lower;
  const ext = base.includes(".") ? base.slice(base.lastIndexOf(".") + 1) : "";

  switch (ext) {
    case "json":
      return [json()];
    case "yml":
    case "yaml":
      return [yaml()];
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
      return [StreamLanguage.define(properties)];
  }

  if (base === "dockerfile" || base.startsWith("dockerfile.")) {
    return [StreamLanguage.define(dockerFile)];
  }
  if (base.includes("nginx") || lower.endsWith("/sites-available") || lower.includes("nginx/")) {
    return [StreamLanguage.define(nginx)];
  }
  if (base.endsWith(".service") || base.endsWith(".socket") || base.endsWith(".timer") || base.endsWith(".unit")) {
    return [StreamLanguage.define(properties)];
  }
  if (base === ".env" || base.startsWith(".env.")) {
    return [StreamLanguage.define(properties)];
  }

  return [];
}
