import { invoke } from "@tauri-apps/api/core";

/**
 * The machine-readable classification the Rust side attaches to every
 * error. Mirrors `errors::ErrorCode`.
 *
 * The six coarse codes are the floor - anything the backend has not yet
 * classified more specifically arrives as one of those and is rendered from
 * `message`, exactly as before. The specific ones exist because the UI does
 * something different with them.
 */
export type ErrorCode =
  | "not_found"
  | "invalid_input"
  | "storage"
  | "connection"
  | "internal"
  | "unauthorized"
  | "permission_denied"
  | "port_in_use"
  | "docker_unavailable"
  | "database_server_unavailable"
  | "timeout"
  | "host_key_mismatch";

/**
 * An error from a Tauri command, with the backend's classification intact.
 *
 * `callCommand` used to reduce `{ kind, message }` to `new Error(message)`,
 * throwing away the discriminator the Rust side went to the trouble of
 * providing. That left every caller string-matching an untranslated Rust
 * sentence, which is why users saw things like
 * "invalid input: containing directory doesn't exist" - accurate, and
 * useless to somebody managing a game server.
 *
 * Extending `Error` rather than replacing it keeps every existing
 * `err instanceof Error` and `err.message` call site working untouched;
 * `code` and `params` are additive.
 */
export class CommandError extends Error {
  readonly code: ErrorCode;
  /** Values a translated message interpolates - a port number, a host. */
  readonly params: Record<string, unknown>;

  constructor(message: string, code: ErrorCode, params: Record<string, unknown> | null) {
    super(message);
    this.name = "CommandError";
    this.code = code;
    this.params = params ?? {};
  }
}

/**
 * Every Rust command returns Result<T, AppError>. Tauri surfaces the Err
 * variant as a rejected promise, so callers only need to catch it once here.
 */
export async function callCommand<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    throw normalizeError(error);
  }
}

function normalizeError(error: unknown): Error {
  if (error instanceof Error) return error;
  if (typeof error === "string") return new Error(error);
  // The Rust side's AppError serializes to { kind, code, params, message }
  // (see errors::AppError's own Serialize impl) - Tauri surfaces that plain
  // object as the rejection value verbatim, not wrapped in a JS Error, so it
  // needs its own unwrap here or every backend error message is lost and
  // every caller sees only its own generic fallback text.
  if (error && typeof error === "object" && "message" in error && typeof (error as { message: unknown }).message === "string") {
    const shaped = error as { message: string; code?: unknown; params?: unknown };
    if (typeof shaped.code === "string") {
      return new CommandError(shaped.message, shaped.code as ErrorCode, (shaped.params ?? null) as Record<string, unknown> | null);
    }
    return new Error(shaped.message);
  }
  return new Error("Unknown error");
}

/**
 * The sentence to show a user for an error.
 *
 * Translates by `code` when there is a translation for it, and falls back to
 * the backend's own English `message` otherwise. The fallback is the
 * important half: it means classifying one more error on the Rust side
 * improves the UI immediately, and *not* classifying one costs nothing -
 * there is never a state where an error has no text at all.
 *
 * `t` is passed in rather than imported so this stays a pure function and
 * callers keep using their own `useTranslation` instance.
 */
export function errorMessage(error: unknown, t: (key: string, options?: Record<string, unknown>) => string): string {
  if (error instanceof CommandError) {
    // i18next's context suffix, used for the one case where an optional
    // value changes the shape of the sentence rather than just filling a
    // slot: "port 25565/tcp is taken" and "...is taken by nginx" are
    // different sentences in every language, and gluing the second half on
    // in code is exactly what makes copy untranslatable.
    const context = typeof error.params.owner === "string" ? "owned" : undefined;
    const translated = t(`errors.${error.code}`, { ...error.params, context, defaultValue: "" });
    if (translated) return translated;
  }
  if (error instanceof Error) return error.message;
  return t("errors.unknown", { defaultValue: "Something went wrong." });
}
