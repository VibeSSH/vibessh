import { describe, expect, it } from "vitest";
import { canonicalIpv4Source, firewallFollowUpWarning, firewallResultWarning } from "./firewallWarnings";
import type { FirewallSyncResult } from "./serverService";

const t = (key: string, options?: Record<string, unknown>) => (options ? `${key} ${JSON.stringify(options)}` : key);

const synced: FirewallSyncResult = { backend: "ufw", active: true, rulesApplied: 3, rulesRemoved: 0, unenforced: false, containerError: null };

describe("firewallResultWarning", () => {
  it("says nothing when both halves landed", () => {
    expect(firewallResultWarning(synced, t)).toBeNull();
    expect(firewallResultWarning(null, t)).toBeNull();
  });

  // The ufw half succeeding is exactly what made this look like success.
  it("warns when the container rules did not land, even though ufw did", () => {
    expect(firewallResultWarning({ ...synced, containerError: "iptables failed" }, t)).toContain("firewallFollowUp.containerFailed");
  });
});

describe("firewallFollowUpWarning", () => {
  it("turns a failed side sync into a warning instead of dropping it", () => {
    const warning = firewallFollowUpWarning({ result: null, error: { kind: "connection", code: "connection", message: "refused", params: null } }, t);
    expect(warning).toContain("firewallFollowUp.syncFailed");
  });

  it("says nothing for a Local application, which has no firewall to sync", () => {
    expect(firewallFollowUpWarning({ result: null, error: null }, t)).toBeNull();
  });
});

describe("canonicalIpv4Source", () => {
  it("spells a source the way the Node reports it back", () => {
    expect(canonicalIpv4Source(" 203.0.113.7 ")).toEqual({ ok: true, value: "203.0.113.7" });
    expect(canonicalIpv4Source("203.0.113.7/32")).toEqual({ ok: true, value: "203.0.113.7" });
    expect(canonicalIpv4Source("10.0.0.0/24")).toEqual({ ok: true, value: "10.0.0.0/24" });
    expect(canonicalIpv4Source("0.0.0.0/0")).toEqual({ ok: true, value: "0.0.0.0/0" });
  });

  it("refuses a network with host bits set, naming the one it probably meant", () => {
    expect(canonicalIpv4Source("10.0.0.5/24")).toEqual({ ok: false, network: "10.0.0.0/24" });
    expect(canonicalIpv4Source("192.168.1.130/25")).toEqual({ ok: false, network: "192.168.1.128/25" });
  });

  it("refuses anything that is not an address", () => {
    for (const hostile of ["10.0.0.0/8; reboot", "$(id)", "10.0.0.0/33", "10.0.0", "256.0.0.1", "01.2.3.4", "1.2.3.4/8/8", ""]) {
      expect(canonicalIpv4Source(hostile).ok, hostile).toBe(false);
    }
  });
});
