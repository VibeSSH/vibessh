import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

/**
 * The card that hands out a token and opens a port.
 *
 * Two things are worth pinning here and neither is cosmetic. The token is
 * the only secret this app ever shows the interface, so it must not be on
 * screen until asked for - this page gets screenshotted and screen-shared.
 * And permission to change things must never survive the endpoint being
 * switched off, or turning it back on later would silently restore a
 * permission nobody granted twice.
 */

const getMcpSettings = vi.fn();
const setMcpSettings = vi.fn();
const rotateMcpToken = vi.fn();

vi.mock("@/services/mcpService", () => ({
  getMcpSettings: () => getMcpSettings(),
  setMcpSettings: (...args: unknown[]) => setMcpSettings(...args),
  rotateMcpToken: () => rotateMcpToken(),
}));

vi.mock("@/stores/toastStore", () => ({ toastSuccess: vi.fn() }));

const { McpCard } = await import("./McpCard");

const TOKEN = "d3adb33f".repeat(8);
const OFF = { enabled: false, allowChanges: false, port: 7422, url: "http://127.0.0.1:7422/mcp", token: TOKEN };
const ON = { ...OFF, enabled: true };

describe("McpCard", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    getMcpSettings.mockResolvedValue(OFF);
    setMcpSettings.mockImplementation((input) => Promise.resolve({ ...OFF, ...input, url: `http://127.0.0.1:${input.port}/mcp` }));
    rotateMcpToken.mockResolvedValue({ ...ON, token: "f00d".repeat(16) });
  });

  /// Nothing is listening, so there is no address and no token to show -
  /// and no "allow changes" either, because permission to change something
  /// that is switched off is not a state worth being able to save.
  it("shows nothing but the switch while the endpoint is off", async () => {
    render(<McpCard />);

    await screen.findByText("Answer assistants on this computer");
    expect(screen.queryByDisplayValue("http://127.0.0.1:7422/mcp")).toBeNull();
    expect(screen.queryByText("Allow changes")).toBeNull();
  });

  /// The token is the only secret this app shows the interface at all. It
  /// is here to be copied, but not to be read over somebody's shoulder.
  it("keeps the token masked until it is asked for", async () => {
    getMcpSettings.mockResolvedValue(ON);
    render(<McpCard />);

    const field = (await screen.findByDisplayValue(TOKEN)) as HTMLInputElement;
    expect(field.type).toBe("password");

    await userEvent.click(screen.getByRole("button", { name: "Show" }));
    expect(((await screen.findByDisplayValue(TOKEN)) as HTMLInputElement).type).toBe("text");
  });

  /// Switching the endpoint off must take the permission with it. Leaving
  /// it set would mean turning the endpoint back on later silently restored
  /// a permission that was granted once, for a session that ended.
  it("withdraws permission to make changes when the endpoint is switched off", async () => {
    getMcpSettings.mockResolvedValue({ ...ON, allowChanges: true });
    render(<McpCard />);

    await screen.findByText("Allow changes");
    await userEvent.click(screen.getByRole("switch", { name: "Answer assistants on this computer" }));

    await waitFor(() => expect(setMcpSettings).toHaveBeenCalledWith({ enabled: false, allowChanges: false, port: 7422 }));
  });

  /// Turning it on does not grant anything by itself - the second answer is
  /// still no until somebody gives it.
  it("grants nothing merely by being switched on", async () => {
    render(<McpCard />);

    await screen.findByText("Answer assistants on this computer");
    await userEvent.click(screen.getByRole("switch", { name: "Answer assistants on this computer" }));

    await waitFor(() => expect(setMcpSettings).toHaveBeenCalledWith({ enabled: true, allowChanges: false, port: 7422 }));
  });

  /// A port already taken is the realistic failure, and the switch has to
  /// go back - it is a claim about whether something is listening, and a
  /// claim that did not come true is false.
  it("puts the switch back and says why when the port cannot be opened", async () => {
    setMcpSettings.mockRejectedValue(new Error("couldn't open the local endpoint on 127.0.0.1:7422: address in use"));
    render(<McpCard />);

    await screen.findByText("Answer assistants on this computer");
    await userEvent.click(screen.getByRole("switch", { name: "Answer assistants on this computer" }));

    await screen.findByText(/address in use/);
    await waitFor(() => expect(screen.getByRole("switch", { name: "Answer assistants on this computer" })).not.toBeChecked());
  });
});
