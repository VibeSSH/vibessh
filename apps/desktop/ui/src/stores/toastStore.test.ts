import { beforeEach, describe, expect, it, vi } from "vitest";
import { toastError, toastSuccess, useToastStore } from "./toastStore";

/**
 * The toast store is how every success and failure in the app reaches the
 * user, and it is a module-level singleton, so its bounds and its cleanup
 * matter more than its size suggests: an unbounded history or a timer that
 * never fires is a leak that only shows up in a long session.
 */
describe("toastStore", () => {
  beforeEach(() => {
    useToastStore.setState({ toasts: [], history: [] });
    vi.useFakeTimers();
  });

  it("shows a toast and records it in history", () => {
    toastSuccess("Application started");

    const { toasts, history } = useToastStore.getState();
    expect(toasts).toHaveLength(1);
    expect(toasts[0]).toMatchObject({ message: "Application started", tone: "success" });
    expect(history[0]).toMatchObject({ message: "Application started", tone: "success" });
  });

  it("distinguishes an error from a success", () => {
    toastError("Couldn't reach the Node");

    expect(useToastStore.getState().toasts[0].tone).toBe("error");
  });

  it("dismisses a toast on its own after the auto-dismiss delay", () => {
    toastSuccess("gone shortly");
    expect(useToastStore.getState().toasts).toHaveLength(1);

    vi.advanceTimersByTime(4000);

    expect(useToastStore.getState().toasts).toHaveLength(0);
  });

  /**
   * Auto-dismiss removes the toast but must keep the history entry - the
   * notification list is a record of what happened, not a mirror of what is
   * currently on screen.
   */
  it("keeps history after the toast itself is dismissed", () => {
    toastSuccess("still remembered");
    vi.advanceTimersByTime(4000);

    expect(useToastStore.getState().toasts).toHaveLength(0);
    expect(useToastStore.getState().history).toHaveLength(1);
  });

  it("dismisses only the toast asked for", () => {
    toastSuccess("first");
    toastSuccess("second");
    const [first] = useToastStore.getState().toasts;

    useToastStore.getState().dismiss(first.id);

    const remaining = useToastStore.getState().toasts;
    expect(remaining).toHaveLength(1);
    expect(remaining[0].message).toBe("second");
  });

  /**
   * The cap is the whole reason history is safe to keep at all: without it
   * a long-running session accumulates every toast forever.
   */
  it("caps history and keeps the most recent entries", () => {
    for (let index = 0; index < 40; index += 1) {
      toastSuccess(`toast ${index}`);
    }

    const { history } = useToastStore.getState();
    expect(history).toHaveLength(30);
    // Newest first.
    expect(history[0].message).toBe("toast 39");
    // Not `.at(-1)`: the project targets ES2021, where it does not exist.
    expect(history[history.length - 1].message).toBe("toast 10");
  });

  it("clears history without touching visible toasts", () => {
    toastSuccess("visible");

    useToastStore.getState().clearHistory();

    expect(useToastStore.getState().history).toHaveLength(0);
    expect(useToastStore.getState().toasts).toHaveLength(1);
  });

  it("gives every toast a distinct id", () => {
    toastSuccess("a");
    toastSuccess("b");
    toastSuccess("c");

    const ids = useToastStore.getState().toasts.map((toast) => toast.id);
    expect(new Set(ids).size).toBe(3);
  });
});
