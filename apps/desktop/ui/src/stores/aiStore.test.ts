import { beforeEach, describe, expect, it, vi } from "vitest";

/**
 * The turn loop, against a fake backend.
 *
 * The Rust side has its own tests for what gets sent and what gets
 * redacted. What only exists here is the half that decides what the user
 * sees while a turn is in flight: that deltas accumulate, that `done`
 * replaces the accumulated text rather than appending to it, that a failure
 * does not leave an empty bubble sitting where an answer should be, and
 * that Stop keeps the partial answer instead of discarding it.
 */

/** Handlers registered by the store, so a test can drive the stream. */
const listeners: {
  phase?: (phase: string) => void;
  delta?: (delta: string) => void;
  done?: (answer: string) => void;
  error?: (payload: unknown) => void;
} = {};

const sendAiTurn = vi.fn(async () => {});
const stopAiTurn = vi.fn(async () => true);

vi.mock("@/services/aiService", () => ({
  sendAiTurn: (...args: unknown[]) => sendAiTurn(...(args as [])),
  stopAiTurn: (...args: unknown[]) => stopAiTurn(...(args as [])),
  onAiPhase: async (_id: string, handler: (phase: string) => void) => {
    listeners.phase = handler;
    return () => {};
  },
  onAiDelta: async (_id: string, handler: (delta: string) => void) => {
    listeners.delta = handler;
    return () => {};
  },
  onAiDone: async (_id: string, handler: (answer: string) => void) => {
    listeners.done = handler;
    return () => {};
  },
  onAiError: async (_id: string, handler: (payload: unknown) => void) => {
    listeners.error = handler;
    return () => {};
  },
  getAiConfig: async () => ({ enabled: true, provider: "openAiCompatible", baseUrl: "http://x/v1", model: "m", hasApiKey: true }),
  previewAiContext: async () => null,
  setAiConfig: async () => ({ enabled: true, provider: "openAiCompatible", baseUrl: "", model: "", hasApiKey: false }),
  testAiConnection: async () => {},
}));

const { useAiStore } = await import("./aiStore");

/** Lets the store's own awaits run before the test inspects the result. */
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

describe("aiStore", () => {
  beforeEach(() => {
    useAiStore.setState({ messages: [], mode: "ask", context: null, contextLabel: null, turnId: null, error: null, pendingQuestion: null, phase: null });
    listeners.phase = undefined;
    listeners.delta = undefined;
    listeners.done = undefined;
    listeners.error = undefined;
    sendAiTurn.mockClear();
    stopAiTurn.mockClear();
  });

  it("shows the question immediately and streams the answer into one bubble", async () => {
    const turn = useAiStore.getState().send("why is it down?");
    await settle();

    const [question, answer] = useAiStore.getState().messages;
    expect(question).toMatchObject({ role: "user", content: "why is it down?" });
    expect(answer).toMatchObject({ role: "assistant", pending: true });

    listeners.delta?.("The port ");
    listeners.delta?.("is taken.");
    expect(useAiStore.getState().messages[1].content).toBe("The port is taken.");

    listeners.done?.("The port is taken.");
    await turn;

    expect(useAiStore.getState().messages[1]).toMatchObject({ content: "The port is taken.", pending: false });
    expect(useAiStore.getState().turnId).toBeNull();
  });

  /**
   * `done` carries the whole answer and is authoritative. If it appended
   * instead of replacing, a dropped delta would show as a hole in the text
   * and a delivered one would show as duplicated words.
   */
  it("takes the final answer as authoritative rather than appending it", async () => {
    const turn = useAiStore.getState().send("hi");
    await settle();
    listeners.delta?.("Hel");
    listeners.done?.("Hello.");
    await turn;

    expect(useAiStore.getState().messages[1].content).toBe("Hello.");
  });

  it("does not start a second turn while one is running", async () => {
    const turn = useAiStore.getState().send("first");
    await settle();
    await useAiStore.getState().send("second");

    expect(sendAiTurn).toHaveBeenCalledTimes(1);
    listeners.done?.("done");
    await turn;
  });

  it("ignores an empty question", async () => {
    await useAiStore.getState().send("   ");
    expect(sendAiTurn).not.toHaveBeenCalled();
    expect(useAiStore.getState().messages).toHaveLength(0);
  });

  /**
   * A failure that arrives before any text would otherwise leave an empty
   * assistant bubble, which reads as the model having answered with silence
   * rather than as something having gone wrong.
   */
  it("removes the empty answer bubble when the turn fails outright", async () => {
    const turn = useAiStore.getState().send("hi");
    await settle();
    listeners.error?.({ kind: "connection", code: "ai_provider_unavailable", params: {}, message: "couldn't reach the AI provider" });
    await turn;

    const state = useAiStore.getState();
    expect(state.messages.map((m) => m.role)).toEqual(["user"]);
    expect(state.error).toBeInstanceOf(Error);
    expect(state.turnId).toBeNull();
  });

  /** The error payload arrives on an event, not as a command rejection, so
   * it has to be normalised for `errorMessage` to translate it by code. */
  it("keeps the backend error code so the panel can translate it", async () => {
    const turn = useAiStore.getState().send("hi");
    await settle();
    listeners.error?.({ kind: "invalid_input", code: "ai_auth_failed", params: {}, message: "the AI provider rejected the API key" });
    await turn;

    expect((useAiStore.getState().error as { code?: string }).code).toBe("ai_auth_failed");
  });

  it("keeps whatever had arrived when the user presses Stop", async () => {
    const turn = useAiStore.getState().send("hi");
    await settle();
    listeners.delta?.("Half an ans");
    await useAiStore.getState().stop();
    await turn;

    const answer = useAiStore.getState().messages[1];
    expect(answer.content).toBe("Half an ans");
    expect(answer.stopped).toBe(true);
    expect(answer.pending).toBe(false);
    expect(useAiStore.getState().turnId).toBeNull();
    expect(stopAiTurn).toHaveBeenCalledTimes(1);
  });

  it("sends the whole conversation so a follow-up question has its history", async () => {
    const first = useAiStore.getState().send("why is it down?");
    await settle();
    listeners.done?.("The port is taken.");
    await first;

    const second = useAiStore.getState().send("by what?");
    await settle();
    listeners.done?.("By nginx.");
    await second;

    const [, request] = sendAiTurn.mock.calls[1] as unknown as [string, { messages: { role: string; content: string }[] }];
    expect(request.messages.map((m) => m.content)).toEqual(["why is it down?", "The port is taken.", "by what?"]);
  });

  /** `ask` promises that nothing is read from the user's infrastructure.
   * The backend enforces it too, but the UI should not be sending a
   * reference it has just promised not to use. */
  it("withholds the context reference in ask mode", async () => {
    useAiStore.setState({ mode: "ask", context: { kind: "node", id: "node-1" } });
    const turn = useAiStore.getState().send("what is a Blueprint?");
    await settle();
    listeners.done?.("A template.");
    await turn;

    const [, request] = sendAiTurn.mock.calls[0] as unknown as [string, { context: unknown }];
    expect(request.context).toBeNull();
  });

  /**
   * The bug this exists for: a Diagnose turn reads the Node over SSH before
   * it ever reaches a model, and both waits used to render as "Thinking…" -
   * so an unresponsive Node looked like a slow model. The phase is what the
   * panel uses to tell them apart, and it must start as collecting rather
   * than waiting for the backend to say so.
   */
  it("reports the collection phase before the model is reached", async () => {
    useAiStore.setState({ mode: "diagnose", context: { kind: "application", id: "app-1" } });
    const turn = useAiStore.getState().send("what is wrong?");
    expect(useAiStore.getState().phase).toBe("collecting");
    await settle();

    listeners.phase?.("waiting");
    expect(useAiStore.getState().phase).toBe("waiting");

    listeners.done?.("Fixed.");
    await turn;
    expect(useAiStore.getState().phase).toBeNull();
  });

  it("seeds a diagnose conversation from a quick action", () => {
    useAiStore.getState().seed({ context: { kind: "application", id: "app-1" }, contextLabel: "paper", question: "what is wrong?" });

    const state = useAiStore.getState();
    expect(state.mode).toBe("diagnose");
    expect(state.contextLabel).toBe("paper");
    expect(state.consumePendingQuestion()).toBe("what is wrong?");
    // Consumed once, so a re-render cannot resend it.
    expect(useAiStore.getState().consumePendingQuestion()).toBeNull();
  });

  it("clears the transcript without touching the running turn", async () => {
    const turn = useAiStore.getState().send("hi");
    await settle();
    useAiStore.getState().clearConversation();
    expect(useAiStore.getState().messages).toHaveLength(0);

    // The turn is still live; its late answer has nowhere to land and must
    // not resurrect the conversation.
    listeners.done?.("too late");
    await turn;
    expect(useAiStore.getState().messages).toHaveLength(0);
  });
});
