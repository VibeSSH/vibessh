import "@testing-library/jest-dom/vitest";
import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import en from "@/i18n/locales/en.json";
import { afterEach, vi } from "vitest";
import { cleanup } from "@testing-library/react";

/**
 * Every test starts from a clean DOM and clean module state.
 *
 * Zustand stores are module-level singletons, so without this a store one
 * test mutated stays mutated for every test after it - the kind of coupling
 * that makes a suite pass in one order and fail in another.
 */
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

/**
 * Component tests run against the **real** English strings rather than a
 * stub that echoes keys back.
 *
 * Two reasons. Assertions then read as what the user actually sees
 * ("Remove", not "common.remove"), so a test failing tells you what broke on
 * screen. And `Trans`-based copy - the delete confirmation interpolates the
 * application's name into a sentence - renders nothing meaningful without a
 * real catalog, so a stub would quietly make those assertions untestable.
 *
 * `src/i18n/index.ts` is deliberately not imported: it installs a
 * `localStorage`/`navigator` language detector, which is device state a test
 * has no business depending on.
 */
void i18n.use(initReactI18next).init({
  lng: "en",
  fallbackLng: "en",
  resources: { en: { translation: en } },
  interpolation: { escapeValue: false },
});

/**
 * `crypto.randomUUID` is used by `toastStore` and is absent from jsdom.
 * A real UUID is not needed - only that ids are unique within a test.
 */
if (!globalThis.crypto?.randomUUID) {
  let counter = 0;
  Object.defineProperty(globalThis, "crypto", {
    value: { ...globalThis.crypto, randomUUID: () => `test-uuid-${counter++}` },
    configurable: true,
  });
}
