import { errorMessage, normalizeError } from "./tauri";
import type { FirewallFollowUp, FirewallSyncResult } from "./serverService";

type Translate = (key: string, options?: Record<string, unknown>) => string;

/**
 * The sentence to show when a firewall sync did not fully land, or `null`
 * when it did.
 *
 * Only what the operator has to act on: a Node with no firewall, or one that
 * is switched off, is shown where the firewall's state is shown, not as a
 * warning on every port save - VibeSSH's own ports are still bound to the
 * right address there. A container restriction that failed to write is
 * different: the panel says the port is Vibe Network only, and it is not.
 */
export function firewallResultWarning(result: FirewallSyncResult | null, t: Translate): string | null {
  if (result?.containerError) return t("firewallFollowUp.containerFailed", { message: result.containerError });
  return null;
}

/** `firewallResultWarning`, for a sync that ran on the side of another change - which can also have failed outright. */
export function firewallFollowUpWarning(followUp: FirewallFollowUp, t: Translate): string | null {
  if (followUp.error) return t("firewallFollowUp.syncFailed", { message: errorMessage(normalizeError(followUp.error), t) });
  return firewallResultWarning(followUp.result, t);
}

/**
 * A custom rule's source in the one spelling the Node reports it back in -
 * the same rule as the Rust `command::canonical_ipv4_source`, checked here
 * first so the refusal is in the reader's language. A host loses its `/32`;
 * a network with host bits set is refused with the network it probably
 * meant, rather than silently becoming a different rule.
 */
export function canonicalIpv4Source(value: string): { ok: true; value: string } | { ok: false; network?: string } {
  const trimmed = value.trim();
  const [address, prefix, ...rest] = trimmed.split("/");
  if (rest.length > 0) return { ok: false };
  const octets = address.split(".");
  if (octets.length !== 4 || !octets.every((octet) => /^\d{1,3}$/.test(octet) && Number(octet) <= 255 && (octet === "0" || !octet.startsWith("0")))) {
    return { ok: false };
  }
  if (prefix === undefined) return { ok: true, value: trimmed };
  if (!/^\d{1,2}$/.test(prefix) || Number(prefix) > 32) return { ok: false };
  const bits = Number(prefix);
  if (bits === 32) return { ok: true, value: address };
  const numeric = octets.reduce((acc, octet) => acc * 256 + Number(octet), 0);
  const size = 2 ** (32 - bits);
  const network = numeric - (numeric % size);
  if (network !== numeric) {
    const dotted = [24, 16, 8, 0].map((shift) => Math.floor(network / 2 ** shift) % 256).join(".");
    return { ok: false, network: `${dotted}/${bits}` };
  }
  return { ok: true, value: `${address}/${bits}` };
}
