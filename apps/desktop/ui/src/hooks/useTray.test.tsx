import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, waitFor } from "@testing-library/react";

/**
 * The two wires between the tray menu and the running interface.
 *
 * The menu is built in Rust, where nothing can see what language `i18next`
 * resolved to, and its "Check for updates..." item deliberately checks
 * nothing itself. Both halves are easy to leave half-connected and neither
 * shows up on screen, so both are pinned here.
 */

const setTrayLanguage = vi.fn();
const checkNow = vi.fn();
const listen = vi.fn();
const unlisten = vi.fn();

vi.mock("@/services/trayService", () => ({
  setTrayLanguage: (...args: unknown[]) => setTrayLanguage(...args),
  TRAY_CHECK_FOR_UPDATES_EVENT: "tray://check-for-updates",
}));

vi.mock("@tauri-apps/api/event", () => ({ listen: (...args: unknown[]) => listen(...args) }));

vi.mock("@/stores/updateStore", () => ({
  useUpdateStore: { getState: () => ({ checkNow }) },
}));

let resolvedLanguage = "pl";
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ i18n: { resolvedLanguage, language: resolvedLanguage } }),
}));

const { useTray } = await import("./useTray");

function Harness() {
  useTray();
  return null;
}

describe("useTray", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    resolvedLanguage = "pl";
    setTrayLanguage.mockResolvedValue(undefined);
    listen.mockResolvedValue(unlisten);
  });

  /// Without this the menu beside the clock is in whatever language the Rust
  /// side guessed, which is English, forever.
  it("tells the Rust side which language the interface settled on", async () => {
    render(<Harness />);
    await waitFor(() => expect(setTrayLanguage).toHaveBeenCalledWith("pl"));
  });

  /// The half that is easy to forget: switching language in Settings has to
  /// reach the menu too, or it stays Polish until the next restart.
  it("tells it again when the language changes", async () => {
    const { rerender } = render(<Harness />);
    await waitFor(() => expect(setTrayLanguage).toHaveBeenCalledWith("pl"));

    resolvedLanguage = "en";
    rerender(<Harness />);
    await waitFor(() => expect(setTrayLanguage).toHaveBeenCalledWith("en"));
  });

  /// A tray menu stuck in the previous language is a blemish; an interface
  /// that failed to start because it could not rename a menu item would be a
  /// fault. So the rejection must not escape.
  it("survives the language call failing", async () => {
    setTrayLanguage.mockRejectedValue(new Error("no tray on this platform"));
    expect(() => render(<Harness />)).not.toThrow();
    await waitFor(() => expect(setTrayLanguage).toHaveBeenCalled());
  });

  /// "Check for updates..." runs the interface's own check - the one that
  /// verifies the installer's signature - rather than a second one living in
  /// the tray.
  it("runs the existing update check when the tray asks for one", async () => {
    render(<Harness />);
    await waitFor(() => expect(listen).toHaveBeenCalledWith("tray://check-for-updates", expect.any(Function)));

    const handler = listen.mock.calls[0][1] as () => void;
    handler();
    expect(checkNow).toHaveBeenCalledTimes(1);
  });

  /// One listener per mount. Without the cleanup, every remount of the shell
  /// adds another, and one tray click eventually fires several checks.
  it("stops listening when the shell goes away", async () => {
    const { unmount } = render(<Harness />);
    await waitFor(() => expect(listen).toHaveBeenCalled());

    unmount();
    await waitFor(() => expect(unlisten).toHaveBeenCalledTimes(1));
  });
});
