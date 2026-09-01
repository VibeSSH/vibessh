import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CommandError as CommandErrorInstance } from "./tauri";
import { expectRejection } from "@/test/expectRejection";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

const { callCommand, CommandError, errorMessage } = await import("./tauri");

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
   * S-019 closed. `AppError` serializes `code` and `params` specifically so
   * the frontend can branch and translate rather than string-matching an
   * untranslated Rust message, and `callCommand` now preserves both.
   */
  it("preserves the error code and params", async () => {
    invoke.mockImplementation(() =>
      Promise.reject({
        kind: "invalid_input",
        code: "port_in_use",
        params: { port: 25565, protocol: "tcp", owner: "nginx" },
        message: "port 25565/tcp is already in use",
      }),
    );

    const error = await expectRejection(callCommand("add_application_port"));
    expect(error).toBeInstanceOf(CommandError);
    expect((error as CommandErrorInstance).code).toBe("port_in_use");
    expect((error as CommandErrorInstance).params).toEqual({ port: 25565, protocol: "tcp", owner: "nginx" });
    // Still an Error, so every existing `instanceof Error` / `.message`
    // call site keeps working untouched.
    expect(error).toBeInstanceOf(Error);
    expect((error as Error).message).toBe("port 25565/tcp is already in use");
  });

  /**
   * The backend has 685 error construction sites and most are still the
   * coarse variants. Those must keep behaving exactly as before rather than
   * becoming a half-migrated special case.
   */
  it("falls back to a plain Error when the backend sent no code", async () => {
    invoke.mockImplementation(() => Promise.reject({ kind: "invalid_input", message: "invalid input: a name is required" }));

    const error = await expectRejection(callCommand("create_application"));
    expect(error).toBeInstanceOf(Error);
    expect(error).not.toBeInstanceOf(CommandError);
    expect((error as Error).message).toBe("invalid input: a name is required");
  });
});

describe("errorMessage", () => {
  // A stand-in for i18next: returns the key so the test can see which one
  // was chosen, and honours `defaultValue` the way i18next does for a key
  // that has no translation.
  function t(key: string, options?: Record<string, unknown>) {
    const known = ["errors.port_in_use", "errors.port_in_use_owned", "errors.timeout", "errors.unknown"];
    const withContext = options?.context ? `${key}_${options.context}` : key;
    if (known.includes(withContext)) return withContext;
    return (options?.defaultValue as string) ?? withContext;
  }

  it("translates by code", () => {
    const error = new CommandError("port 25565/tcp is already in use", "port_in_use", { port: 25565 });
    expect(errorMessage(error, t)).toBe("errors.port_in_use");
  });

  /**
   * "port X is taken" and "port X is taken by nginx" are different
   * sentences in every language - gluing the second half on in code is
   * exactly what makes copy untranslatable, so the owner selects a
   * different key rather than being appended.
   */
  it("uses the owned variant when something holds the port", () => {
    const error = new CommandError("...", "port_in_use", { port: 25565, owner: "nginx" });
    expect(errorMessage(error, t)).toBe("errors.port_in_use_owned");
  });

  /**
   * The important half of the design: classifying one more error on the
   * Rust side improves the UI immediately, and *not* classifying one costs
   * nothing - there is never a state where an error has no text.
   */
  it("falls back to the backend message for a code with no translation yet", () => {
    const error = new CommandError("storage error: database is locked", "storage", {});
    expect(errorMessage(error, t)).toBe("storage error: database is locked");
  });

  it("falls back to a plain Error's message", () => {
    expect(errorMessage(new Error("the webview died"), t)).toBe("the webview died");
  });

  it("has something to say even for a non-Error", () => {
    expect(errorMessage({ unexpected: true }, t)).toBe("errors.unknown");
  });
});
