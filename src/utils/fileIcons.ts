/**
 * Which icon a file gets, and what shade to draw it in.
 *
 * **Why not one icon for everything.** Every file used to render the same
 * generic sheet, so a directory called `landmc-auth` and a file called
 * `landmc-auth.jar` differed by the shape of a small grey outline and nothing
 * else. In a plugins directory - dozens of pairs named exactly that way -
 * there was no shape for the eye to catch, and finding anything meant reading
 * every row.
 *
 * The tone does most of the work: colour is what separates rows at a glance,
 * where an outline has to be looked at. It stays deliberately muted - these
 * are dozens of small marks down the left edge of a list, and a column of
 * saturated colour is harder to read than the grey it replaced, not easier.
 */
export type FileTone = "folder" | "archive" | "config" | "data" | "script" | "secret" | "log" | "plain";

export interface FileIcon {
  /** A key in the Icon component's own map - the coverage test checks these exist. */
  name: string;
  tone: FileTone;
}

const BY_EXTENSION: Record<string, FileIcon> = {
  // A jar is the thing this list is mostly made of on a Minecraft server, so
  // it gets a mark of its own rather than sharing the archive one.
  jar: { name: "box", tone: "archive" },

  zip: { name: "archive", tone: "archive" },
  gz: { name: "archive", tone: "archive" },
  tgz: { name: "archive", tone: "archive" },
  tar: { name: "archive", tone: "archive" },
  rar: { name: "archive", tone: "archive" },
  "7z": { name: "archive", tone: "archive" },

  yml: { name: "settings", tone: "config" },
  yaml: { name: "settings", tone: "config" },
  json: { name: "settings", tone: "config" },
  toml: { name: "settings", tone: "config" },
  conf: { name: "settings", tone: "config" },
  cfg: { name: "settings", tone: "config" },
  ini: { name: "settings", tone: "config" },
  properties: { name: "settings", tone: "config" },
  env: { name: "settings", tone: "config" },

  db: { name: "database", tone: "data" },
  sqlite: { name: "database", tone: "data" },
  sqlite3: { name: "database", tone: "data" },
  mca: { name: "database", tone: "data" },
  dat: { name: "database", tone: "data" },

  sh: { name: "terminal", tone: "script" },
  bash: { name: "terminal", tone: "script" },
  bat: { name: "terminal", tone: "script" },
  cmd: { name: "terminal", tone: "script" },
  ps1: { name: "terminal", tone: "script" },

  pem: { name: "key", tone: "secret" },
  key: { name: "key", tone: "secret" },
  crt: { name: "key", tone: "secret" },
  cer: { name: "key", tone: "secret" },

  log: { name: "history", tone: "log" },
};

const FOLDER: FileIcon = { name: "folder", tone: "folder" };
const PLAIN: FileIcon = { name: "file", tone: "plain" };

export function fileIcon(name: string, isDir: boolean): FileIcon {
  if (isDir) return FOLDER;

  // The last dot, so `paper-1.21.11.jar` is a jar rather than an unknown
  // `21` - and a dotfile like `.gitignore` has no extension at all rather
  // than an extension of "gitignore".
  const dot = name.lastIndexOf(".");
  if (dot <= 0) return PLAIN;

  return BY_EXTENSION[name.slice(dot + 1).toLowerCase()] ?? PLAIN;
}
