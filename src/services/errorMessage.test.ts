import { describe, expect, it } from "vitest";
import { CommandError, errorMessage } from "./tauri";
import pl from "@/i18n/locales/pl.json";
import en from "@/i18n/locales/en.json";

type Bundle = Record<string, Record<string, string>>;

/** Stands in for i18next: finds the key, fills `{{slots}}`, and returns the
 * empty string for a key it does not have - which is what `defaultValue: ""`
 * makes the real one do, and what `errorMessage` branches on. */
function translateWith(bundle: Bundle) {
  return (key: string, options?: Record<string, unknown>): string => {
    const [namespace, name] = key.split(".");
    const template = bundle[namespace]?.[name];
    if (template === undefined) return (options?.defaultValue as string) ?? key;
    return template.replace(/\{\{(\w+)\}\}/g, (_, slot) => String(options?.[slot] ?? ""));
  };
}

/**
 * Every error a user can see reaches them in their own language.
 *
 * The mechanism was always there - errors carry a code and the interface
 * translates by it - but the six coarse codes had no entry, so anything
 * raised as one of those fell through to the English `Display` text. A user
 * reported "unauthorized: not signed in to the VibeSSH cloud backend" showing
 * up untranslated in a Polish interface.
 */
describe("errorMessage", () => {
  const GENERIC = ["not_found", "invalid_input", "storage", "connection", "internal", "unauthorized"] as const;

  it.each(GENERIC)("translates the %s code rather than showing the English message", (code) => {
    const error = new CommandError("some english detail", code, { message: "jakiś szczegół" });

    const message = errorMessage(error, translateWith(pl as unknown as Bundle));

    expect(message).not.toBe("some english detail");
    expect(message).toContain("jakiś szczegół");
  });

  /** The reported case, end to end: this one gets a whole sentence rather
   * than a frame, because the detail adds nothing a user can act on. */
  it("says in Polish that nobody is signed in", () => {
    const error = new CommandError("not signed in to the VibeSSH cloud backend", "not_signed_in", { message: "not signed in to the VibeSSH cloud backend" });

    const message = errorMessage(error, translateWith(pl as unknown as Bundle));

    expect(message).toBe("Nie jesteś zalogowany do backendu kont. Zaloguj się albo ustaw jego adres w Ustawieniach.");
    expect(message).not.toContain("VibeSSH cloud backend");
  });

  it("has the same codes in English", () => {
    for (const code of [...GENERIC, "not_signed_in"] as const) {
      const message = errorMessage(new CommandError("detail", code, { message: "detail" }), translateWith(en as unknown as Bundle));
      expect(message).toBeTruthy();
    }
  });

  /** A code with no entry still has to say something, rather than rendering
   * an empty string where the error was. */
  it("falls back to the message for a code nothing has translated yet", () => {
    const error = new CommandError("a brand new failure", "port_in_use", {});
    expect(errorMessage(error, translateWith({} as Bundle))).toBe("a brand new failure");
  });
});
