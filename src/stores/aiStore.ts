import { create } from "zustand";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { onAiDelta, onAiDone, onAiError, sendAiTurn, stopAiTurn } from "@/services/aiService";
import { normalizeError } from "@/services/tauri";
import type { AiContextRef, AiMessage, AiMode } from "@/types/ai";

/**
 * The conversation, and the machinery for one turn.
 *
 * **Why the send loop lives in the store rather than in the page.** A turn
 * can take a minute. If it were owned by the component, navigating away
 * mid-answer would unmount the listeners and lose a reply the user is still
 * being charged for. Here the stream keeps writing into the store and the
 * answer is waiting when they come back. It is also what lets the "Ask Vibe
 * AI" button on an error seed a conversation from a different page entirely.
 *
 * **Nothing is persisted.** The conversation lives for as long as the window
 * does, matching the Rust side, which also keeps no transcript. A
 * conversation about a broken Node is a transcript of that Node's
 * configuration and logs; putting it in `localStorage` would create a
 * durable copy of exactly what the rest of this feature works to contain.
 */

export interface AiChatMessage {
  id: string;
  role: "user" | "assistant";
  content: string;
  /** Still streaming. The bubble shows a caret and Send stays disabled. */
  pending?: boolean;
  /** The user pressed Stop. Whatever arrived is kept, and marked as partial. */
  stopped?: boolean;
}

/**
 * The turn currently streaming, so Stop can end the wait `send` is parked
 * on. Module-level rather than in the store because it is a function, not
 * something anything renders - and because a turn that ends by being
 * aborted produces no `done` and no `error` event, so there has to be a
 * third way for `send` to unwind and drop its listeners.
 */
let activeTurn: { id: string; finish: () => void } | null = null;

interface AiState {
  messages: AiChatMessage[];
  mode: AiMode;
  context: AiContextRef | null;
  /** A human name for the context - "paper-survival", "vps-test". Kept
   * alongside the reference so the panel can say what it is looking at
   * without another round trip for a name it was already shown. */
  contextLabel: string | null;
  /** The turn in flight, or null. Doubles as the busy flag. */
  turnId: string | null;
  /** The last failure, unrendered - the page turns it into a sentence with
   * its own `t`, so this stays free of translation concerns. */
  error: unknown | null;
  /** Seeded by a quick action, consumed by the panel on mount. */
  pendingQuestion: string | null;

  setMode: (mode: AiMode) => void;
  setContext: (context: AiContextRef | null, label: string | null) => void;
  clearConversation: () => void;
  send: (question: string) => Promise<void>;
  stop: () => Promise<void>;
  consumePendingQuestion: () => string | null;
  /** Opens a conversation about something specific, from anywhere in the app. */
  seed: (input: { context: AiContextRef; contextLabel: string; question: string }) => void;
}

function newId(): string {
  return crypto.randomUUID();
}

export const useAiStore = create<AiState>((set, get) => ({
  messages: [],
  mode: "ask",
  context: null,
  contextLabel: null,
  turnId: null,
  error: null,
  pendingQuestion: null,

  setMode: (mode) => set({ mode }),
  setContext: (context, label) => set({ context, contextLabel: label }),

  clearConversation: () => {
    // Deliberately does not stop a running turn - Clear and Stop are
    // separate buttons and mean separate things. A turn left running writes
    // into a message id that no longer exists, which `updateAnswer` handles
    // by doing nothing.
    set({ messages: [], error: null });
  },

  consumePendingQuestion: () => {
    const question = get().pendingQuestion;
    if (question !== null) set({ pendingQuestion: null });
    return question;
  },

  seed: ({ context, contextLabel, question }) =>
    set({
      context,
      contextLabel,
      // A quick action is by definition about something specific, so it
      // arrives in the mode that actually looks at it.
      mode: "diagnose",
      messages: [],
      error: null,
      pendingQuestion: question,
    }),

  send: async (question) => {
    const trimmed = question.trim();
    if (!trimmed || get().turnId) return;

    const turnId = newId();
    const answerId = newId();
    const userMessage: AiChatMessage = { id: newId(), role: "user", content: trimmed };
    const answer: AiChatMessage = { id: answerId, role: "assistant", content: "", pending: true };

    const history: AiMessage[] = [...get().messages, userMessage].map((message) => ({ role: message.role, content: message.content }));
    set({ messages: [...get().messages, userMessage, answer], turnId, error: null });

    /** Writes into the streaming bubble, or does nothing if it is gone -
     * which is what happens when the conversation was cleared mid-turn. */
    const updateAnswer = (change: (message: AiChatMessage) => AiChatMessage) =>
      set((state) => ({ messages: state.messages.map((message) => (message.id === answerId ? change(message) : message)) }));

    let release: () => void = () => {};
    const finished = new Promise<void>((resolve) => {
      release = resolve;
    });
    const finish = () => {
      if (activeTurn?.id === turnId) activeTurn = null;
      release();
    };
    activeTurn = { id: turnId, finish };

    let unlisteners: UnlistenFn[] = [];
    try {
      unlisteners = await Promise.all([
        onAiDelta(turnId, (delta) => updateAnswer((message) => ({ ...message, content: message.content + delta }))),
        // `done` carries the whole answer and is authoritative: a delta
        // dropped by a busy event loop would otherwise leave a hole in the
        // middle of the text with nothing to show it had been there.
        onAiDone(turnId, (whole) => {
          updateAnswer((message) => ({ ...message, content: whole, pending: false }));
          finish();
        }),
        onAiError(turnId, (payload) => {
          set({ error: normalizeError(payload) });
          updateAnswer((message) => ({ ...message, pending: false }));
          finish();
        }),
      ]);

      await sendAiTurn(turnId, { mode: get().mode, context: get().mode === "diagnose" ? get().context : null, messages: history });
      await finished;
    } catch (err) {
      // A synchronous refusal - the assistant is not configured, or the
      // config directory could not be read. No events will arrive.
      set({ error: err });
      updateAnswer((message) => ({ ...message, pending: false }));
      finish();
    } finally {
      unlisteners.forEach((off) => off());
      if (activeTurn?.id === turnId) activeTurn = null;
      set((state) => (state.turnId === turnId ? { turnId: null } : {}));
      // An answer that ended with nothing in it - an immediate provider
      // error - leaves an empty bubble, which reads as the model having
      // said nothing rather than as a failure. The error banner carries the
      // explanation, so the placeholder goes.
      set((state) => ({
        messages: state.messages.filter((message) => !(message.id === answerId && message.content === "" && !message.pending)),
      }));
    }
  },

  stop: async () => {
    const turnId = get().turnId;
    if (!turnId) return;
    try {
      await stopAiTurn(turnId);
    } catch {
      // A failed stop is not worth an error banner: either the request had
      // already finished or the backend is gone, and in both cases the right
      // thing for the panel to do is stop waiting.
    }
    set((state) => ({
      messages: state.messages.map((message) => (message.pending ? { ...message, pending: false, stopped: true } : message)),
    }));
    // No `done` or `error` event follows an abort, so this is what lets
    // `send` unwind and drop its listeners.
    activeTurn?.finish();
  },
}));
