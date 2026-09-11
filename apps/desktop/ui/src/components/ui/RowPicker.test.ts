import { describe, expect, it } from "vitest";
import { serverRowPickerOption } from "./RowPicker";
import pl from "@/i18n/locales/pl.json";
import en from "@/i18n/locales/en.json";

type Bundle = Record<string, Record<string, string>>;

/** What i18next does with a key it cannot find: it returns the key. So a
 * lookup into the real bundle is the only way to tell a translated label
 * from a missing one. */
function translateWith(bundle: Bundle) {
  return (key: string): string => {
    const [namespace, name] = key.split(".");
    return bundle[namespace]?.[name] ?? key;
  };
}

const STATUSES = ["online", "offline", "connecting", "unknown"] as const;

/**
 * The status shown beside a server in every picker.
 *
 * This shipped broken: the label's key was assembled from the status at
 * runtime - `rail.status` plus a capitalised status - and pointed at a
 * namespace that had no such keys, so the Add Node dialog offered three
 * servers each labelled "rail.statusOnline". Nothing failed; i18next hands
 * back the key it could not find and the interface printed it.
 *
 * These assert against the real locale files rather than a stub, because the
 * bug was not in the function's logic - it was in whether the key it produced
 * exists at all.
 */
describe("serverRowPickerOption", () => {
  const server = { id: "1", name: "Landmc", host: "83.168.69.143" };

  it.each(STATUSES)("translates the %s status in Polish", (status) => {
    const option = serverRowPickerOption({ ...server, status }, translateWith(pl as unknown as Bundle));

    expect(option.status?.label).toBeTruthy();
    expect(option.status?.label).not.toMatch(/^[a-z]+\.[a-zA-Z]+$/);
  });

  it.each(STATUSES)("translates the %s status in English", (status) => {
    const option = serverRowPickerOption({ ...server, status }, translateWith(en as unknown as Bundle));

    expect(option.status?.label).not.toMatch(/^[a-z]+\.[a-zA-Z]+$/);
  });

  /** The tone is what colours the pill, and it is the half that was right all
   * along - pinned so a rewrite of the label lookup cannot take it with it. */
  it("keeps a tone per status", () => {
    const t = translateWith(en as unknown as Bundle);
    expect(serverRowPickerOption({ ...server, status: "online" }, t).status?.tone).toBe("success");
    expect(serverRowPickerOption({ ...server, status: "offline" }, t).status?.tone).toBe("danger");
    expect(serverRowPickerOption({ ...server, status: "connecting" }, t).status?.tone).toBe("warning");
    expect(serverRowPickerOption({ ...server, status: "unknown" }, t).status?.tone).toBe("neutral");
  });

  it("carries the name and host through to the row", () => {
    const option = serverRowPickerOption({ ...server, status: "online" }, translateWith(en as unknown as Bundle));

    expect(option.name).toBe("Landmc");
    expect(option.meta).toBe("83.168.69.143");
  });
});
