import { describe, expect, it } from "vitest";
import { networkWarningMessages } from "./networkWarnings";
import { useServersStore } from "@/stores/serversStore";
import enLocale from "@/i18n/locales/en.json";
import plLocale from "@/i18n/locales/pl.json";
import type { NetworkWarning } from "@/types/network";

const t = (key: string, options?: Record<string, unknown>) => (options ? `${key} ${JSON.stringify(options)}` : key);

const every: NetworkWarning[] = [
  { kind: "peerNotUpdated", serverId: "s1", message: "unreachable" },
  { kind: "noHandshake" },
  { kind: "firewall", message: "iptables failed" },
  { kind: "dns", serverId: "s1", message: "flock" },
  { kind: "dns", serverId: null, message: "duplicate name" },
  { kind: "bindAddresses", message: "storage" },
];

describe("networkWarningMessages", () => {
  it("gives every warning its own sentence, naming the other node", () => {
    useServersStore.setState({ servers: [{ id: "s1", name: "Frankfurt" }] as never });
    const messages = networkWarningMessages(every, t);
    expect(messages).toHaveLength(every.length);
    expect(messages[0]).toContain("vibeNetworkWarnings.peerNotUpdated");
    expect(messages[0]).toContain("Frankfurt");
    expect(messages[4]).toContain("vibeNetworkWarnings.dnsNotRun");
  });

  // A key missing in one language renders as the raw key in that language.
  it("has a translation for every key it uses, in both languages", () => {
    const keys = networkWarningMessages(every, (key) => key);
    for (const locale of [enLocale, plLocale]) {
      for (const key of keys) {
        const [section, name] = key.split(".");
        expect((locale as unknown as Record<string, Record<string, unknown>>)[section]?.[name], key).toBeTruthy();
      }
    }
  });
});
