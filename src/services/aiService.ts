import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { callCommand } from "./tauri";
import type { AiConfigView, AiContextBundle, AiContextRef, AiMode, AiTurnRequest, SetAiConfigInput } from "@/types/ai";

export function getAiConfig(): Promise<AiConfigView> {
  return callCommand<AiConfigView>("get_ai_config");
}

export function setAiConfig(input: SetAiConfigInput): Promise<AiConfigView> {
  return callCommand<AiConfigView>("set_ai_config", { input });
}

/**
 * One real completion against the configured endpoint, so this proves the
 * URL, the key and the model name together. A `GET /models` probe would be
 * cheaper and would pass while the configured model does not exist, which is
 * the failure people actually hit.
 */
export function testAiConnection(): Promise<void> {
  return callCommand<void>("test_ai_connection");
}

/**
 * Exactly what a Diagnose turn would send, collected but not sent.
 *
 * The panel shows this before the first message so the user can see what
 * leaves their machine. It calls the same builder the turn itself calls, so
 * the preview cannot drift from the payload.
 */
export function previewAiContext(mode: AiMode, context: AiContextRef | null): Promise<AiContextBundle | null> {
  return callCommand<AiContextBundle | null>("preview_ai_context", { mode, context });
}

/**
 * Starts a turn. Resolves as soon as the work is running on the Rust side;
 * the answer arrives on the three events below, keyed by `turnId`.
 */
export function sendAiTurn(turnId: string, request: AiTurnRequest): Promise<void> {
  return callCommand<void>("send_ai_turn", { turnId, request });
}

/** `false` means the turn had already finished - a race, not a failure. */
export function stopAiTurn(turnId: string): Promise<boolean> {
  return callCommand<boolean>("stop_ai_turn", { turnId });
}

/** Same not-in-a-Tauri-webview guard as terminalService's helpers - see pairingService for why. */
export function onAiDelta(turnId: string, handler: (delta: string) => void): Promise<UnlistenFn> {
  return listen<string>(`ai://${turnId}/delta`, (event) => handler(event.payload)).catch(() => () => {});
}

export function onAiDone(turnId: string, handler: (answer: string) => void): Promise<UnlistenFn> {
  return listen<string>(`ai://${turnId}/done`, (event) => handler(event.payload)).catch(() => () => {});
}

/**
 * Which of a turn's two waits is currently happening: `collecting` while the
 * Node is being read over SSH, `waiting` once the request is with the model.
 *
 * Both used to render as "Thinking...", which made an unresponsive Node look
 * like a slow model - the panel said the model was thinking when the request
 * had not reached one.
 */
export function onAiPhase(turnId: string, handler: (phase: string) => void): Promise<UnlistenFn> {
  return listen<string>(`ai://${turnId}/phase`, (event) => handler(event.payload)).catch(() => () => {});
}

/**
 * The payload is a serialized `AppError`, so it carries `code` and `params`
 * and the panel renders a translated sentence. It is never the provider's own
 * response body - that stops in Rust and goes to the log.
 */
export function onAiError(turnId: string, handler: (error: unknown) => void): Promise<UnlistenFn> {
  return listen<unknown>(`ai://${turnId}/error`, (event) => handler(event.payload)).catch(() => () => {});
}
