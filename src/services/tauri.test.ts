import { beforeEach, describe, expect, it, vi } from "vitest";
import { expectRejection } from "@/test/expectRejection";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

const { callCommand } = await import("./tauri");

/**
 * This is the single boundary every backend error crosses on its way to the
 * user, so what it preserves decides what the UI is capable of showing.
 * AUDIT_REPORT S-019 is about exactly this function.
 */
describe("callCommand", () => {
  // Braces matter: `mockReset()` returns the mock itself, and vitest treats
  // a function returned from `beforeEach` as a teardown callback. Without
  // them, vitest calls `invoke()` after every test, producing a rejected
  // promise nobody handles - which surfaces as a baffling top-level
  // "Unknown Error" naming the previous test's message.
  beforeEach(() => {
    invoke.mockReset();
  });

  it("passes the command name and arguments through unchanged", async () => {
    invoke.mockImplementation(() => Promise.resolve(["a", "b"]));

    await expect(callCommand<string[]>("list_servers", { id: "s1" })).resolves.toEqual(["a", "b"]);
    expect(invoke).toHaveBeenCalledWith("list_servers", { id: "s1" });
  });

  it("works for a command with no arguments", async () => {
    invoke.mockImplementation(() => Promise.resolve(null));

    await callCommand("cloud_logout");
    expect(invoke).toHaveBeenCalledWith("cloud_logout", undefined);
  });

  /**
   * Tauri surfaces a rejected `Result` as the plain serialized object, not
   * as an `Error`. Losing this unwrap is not cosmetic: every caller falls
   * back to its own generic "something went wrong" string, so the actual
   * reason never reaches the user at all.
   */
  it("unwraps the AppError object Tauri rejects with", async () => {
    invoke.mockImplementation(() => Promise.reject({ kind: "invalid_input", message: "invalid input: a name is required" }));

    const error = await expectRejection(callCommand("create_application"));
    expect(error).toBeInstanceOf(Error);
    expect((error as Error).message).toBe("invalid input: a name is required");
  });

  it("passes a real Error through untouched", async () => {
    const original = new Error("the webview died");
    invoke.mockImplementation(() => Promise.reject(original));

    expect(await expectRejection(callCommand("get_app_info"))).toBe(original);
  });

  it("wraps a bare string rejection", async () => {
    invoke.mockImplementation(() => Promise.reject("something went wrong"));

    const error = await expectRejection(callCommand("get_app_info"));
    expect((error as Error).message).toBe("something went wrong");
  });

  it("never rejects with a non-Error, whatever came back", async () => {
    for (const value of [null, undefined, 42, { unexpected: true }, []]) {
      invoke.mockImplementation(() => Promise.reject(value));
      expect(await expectRejection(callCommand("get_app_info"))).toBeInstanceOf(Error);
    }
  });

  /**
   * Pins the S-019 gap so it stays visible rather than being rediscovered.
   * `AppError` serializes as `{ kind, message }` specifically so the
   * frontend can branch on `kind` - route `unauthorized` to a login prompt,
   * offer a retry for `connection`, translate the rest - and this function
   * throws `kind` away, leaving callers to string-match an untranslated
   * Rust message. Fixing that is FIX_PLAN Phase E (E.1/E.2); until then
   * this documents the current, lossy contract.
   */
  it("currently discards the error kind (AUDIT S-019, Phase E)", async () => {
    invoke.mockImplementation(() => Promise.reject({ kind: "unauthorized", message: "unauthorized: not signed in" }));

    const error = await expectRejection(callCommand("cloud_list_teams"));
    expect(error).toBeInstanceOf(Error);
    expect(error).not.toHaveProperty("kind");
  });
});
