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
  return new Error("Unknown error");
}
