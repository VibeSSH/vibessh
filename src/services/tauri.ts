import { invoke } from "@tauri-apps/api/core";

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
  // The Rust side's AppError serializes to { kind, message } (see
  // errors::AppError's own Serialize impl) - Tauri surfaces that plain
  // object as the rejection value verbatim, not wrapped in a JS Error, so
  // it needs its own unwrap here or every backend error message is lost
  // and every caller sees only its own generic fallback text.
  if (error && typeof error === "object" && "message" in error && typeof (error as { message: unknown }).message === "string") {
    return new Error((error as { message: string }).message);
  }
  return new Error("Unknown error");
}
