import { invoke } from "@tauri-apps/api/core";
import { isRejectedPromptDismissed, useSessionPasswordStore } from "@/stores/sessionPasswordStore";
import { recordCommandTiming } from "./commandTiming";

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
  | "not_signed_in"
  | "permission_denied"
  | "port_in_use"
  | "docker_unavailable"
  | "database_server_unavailable"
  | "timeout"
  | "host_key_mismatch"
  | "password_required"
  | "ssh_auth_rejected"
  | "cron_missing"
  // Vibe AI. Four codes rather than one because the remedy differs: fix the
  // settings, replace the key, correct the model name, or wait and retry.
  | "ai_not_configured"
  | "ai_auth_failed"
  | "ai_model_unavailable"
  | "ai_rate_limited"
  | "ai_provider_unavailable"
  | "ai_hosted_unavailable"
  | "ai_hosted_failed"
  | "ai_quota_exhausted";

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
export async function callCommand<T>(command: string, args?: Record<string, unknown>, isRetry = false): Promise<T> {
  // Measured here because this is the one place every command passes
  // through - see `commandTiming` for why "the UI feels slow" needs numbers
  // before it needs a fix.
  const started = performance.now();
  try {
    const result = await invoke<T>(command, args);
    recordCommandTiming(command, performance.now() - started, false);
    return result;
  } catch (error) {
    recordCommandTiming(command, performance.now() - started, true);
    const normalized = normalizeError(error);

    // A Node that authenticates with a password, on a machine whose keyring
    // holds none - or has no keyring at all. Handled here because this is the
    // one place every command passes through: asking at each call site would
    // mean touching every screen that can open a connection, and missing one
    // would leave a dead end.
    //
    // Retried once only. If the password was wrong the second failure is a
    // real authentication error and belongs to the caller.
    if (!isRetry && normalized instanceof CommandError && normalized.code === "password_required") {
      const serverId = typeof normalized.params.serverId === "string" ? normalized.params.serverId : null;
      if (serverId) {
        const password = await useSessionPasswordStore.getState().request(serverId);
        if (password) {
          await callCommand<void>("remember_session_password", { serverId, password }, true);
          return callCommand<T>(command, args, true);
        }
      }
    }

    // The Node refused the login. Handled here for the same reason as a
    // missing password - every screen reaches a Node through this function -
    // and it used to surface only as a line of English wherever the failure
    // happened to land.
    if (!isRetry && normalized instanceof CommandError && normalized.code === "ssh_auth_rejected") {
      return retryAfterRejectedLogin<T>(normalized, command, args);
    }

    throw normalized;
  }
}

/**
 * Asks for the right password and tries again, for as long as the person
 * keeps typing one.
 *
 * Unlike the missing-password case this does not stop after one attempt: a
 * typo is the usual reason a password is refused, and sending somebody back
 * to the screen after one would be the dead end this exists to remove. Cancel
 * ends it - and keeps it ended on the next poll, see
 * `isRejectedPromptDismissed`. A refused key cannot be retyped, so for one the
 * prompt only points at the server's settings and the error goes on to the
 * caller.
 */
async function retryAfterRejectedLogin<T>(first: CommandError, command: string, args?: Record<string, unknown>): Promise<T> {
  let error: Error = first;
  for (;;) {
    if (!(error instanceof CommandError) || error.code !== "ssh_auth_rejected") throw error;
    const serverId = typeof error.params.serverId === "string" ? error.params.serverId : null;
    if (!serverId || isRejectedPromptDismissed(serverId)) throw error;
    const username = typeof error.params.username === "string" ? error.params.username : undefined;

    if (error.params.method !== "password") {
      await useSessionPasswordStore.getState().request(serverId, { reason: "rejectedKey", username });
      throw error;
    }

    const password = await useSessionPasswordStore.getState().request(serverId, { reason: "rejected", username });
    if (!password) throw error;
    await callCommand<void>("replace_ssh_password", { serverId, password }, true);
    try {
      return await callCommand<T>(command, args, true);
    } catch (retryError) {
      error = normalizeError(retryError);
    }
  }
}

/**
 * Turns whatever Tauri handed back into an `Error` - a `CommandError` when
 * the payload carries the Rust side's `code`.
 *
 * Exported because command rejections are not the only way a backend error
 * reaches the UI: the Vibe AI assistant delivers a failed turn on an *event*
 * (`ai://{id}/error`), whose payload is the same serialized `AppError` but
 * arrives as a plain object that never passed through `callCommand`. Without
 * this, such an error would lose its `code` and fall back to English prose.
 */
export function normalizeError(error: unknown): Error {
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
    // The cloud backend names its own refusals, and its name for one is more
    // specific than the coarse code it also maps to: "invalid_credentials"
    // rather than "unauthorized". Tried first so the user reads one sentence
    // in their own language instead of a translated frame around English
    // prose - which is what "Brak uprawnien: invalid email or password" was.
    // Unknown to this build (a newer backend) falls straight through.
    const backendCode = error.params.backendCode;
    if (typeof backendCode === "string") {
      const fromBackend = t(`cloudErrors.${backendCode}`, { ...error.params, defaultValue: "" });
      if (fromBackend) return fromBackend;
    }
    // `message` first so a key like `errors.internal` ("...: {{message}}")
    // always has something to interpolate - the error's own message - even
    // when the backend did not repeat it inside `params`. A real `params.message`
    // still wins by coming after.
    const translated = t(`errors.${error.code}`, { message: error.message, ...error.params, context, defaultValue: "" });
    if (translated) return translated;
  }
  if (error instanceof Error) return error.message;
  return t("errors.unknown", { defaultValue: "Something went wrong." });
}
