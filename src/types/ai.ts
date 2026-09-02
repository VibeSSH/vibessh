/**
 * Mirrors the Rust `models::ai` types.
 *
 * There is deliberately no `apiKey` on `AiConfig`. The key is written once
 * through `SetAiConfigInput` and never comes back - `hasApiKey` is the whole
 * of what the UI is told about it, which is all it needs to choose between
 * "enter a key" and "a key is stored".
 */

export type AiProviderKind = "openAiCompatible";

export interface AiConfig {
  enabled: boolean;
  provider: AiProviderKind;
  /** API root, e.g. `https://openrouter.ai/api/v1` - not the full endpoint. */
  baseUrl: string;
  model: string;
}

export interface AiConfigView extends AiConfig {
  hasApiKey: boolean;
}

export interface SetAiConfigInput extends AiConfig {
  /** Blank means "leave the stored key alone" - the frontend never holds the
   * real one, so it cannot resend it. */
  apiKey: string;
}

export type AiRole = "user" | "assistant";

/** One turn as the backend wants it. */
export interface AiMessage {
  role: AiRole;
  content: string;
}

/**
 * `ask` answers questions and reads nothing from the user's infrastructure.
 * `diagnose` additionally attaches a snapshot of the referenced Node or
 * Application. The backend enforces this, not the UI - an `ask` turn cannot
 * be talked into collecting a context by sending one anyway.
 */
export type AiMode = "ask" | "diagnose";

export type AiContextRef = { kind: "application"; id: string } | { kind: "node"; id: string };

/** What was collected, exactly as it will be sent. */
export interface AiContextBundle {
  summary: string;
  sources: string[];
  /** What could not be collected. Shown to the user and sent to the model,
   * so it can say what it does not know rather than guessing. */
  notes: string[];
}

export interface AiTurnRequest {
  mode: AiMode;
  context: AiContextRef | null;
  messages: AiMessage[];
}
