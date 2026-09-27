import { useServersStore } from "@/stores/serversStore";
import type { NetworkWarning } from "@/types/network";

type Translate = (key: string, options?: Record<string, unknown>) => string;

/**
 * One sentence per thing a join or a leave did not finish.
 *
 * The membership change itself stands - these are what it takes to trust
 * it: another Node that was not told, a tunnel nobody answered, a firewall
 * or DNS that did not follow. They used to be log lines, so a join that
 * "succeeded" could leave a Node nothing could reach.
 */
export function networkWarningMessages(warnings: NetworkWarning[], t: Translate): string[] {
  const nameOf = (serverId: string | null | undefined) =>
    (serverId && useServersStore.getState().servers.find((server) => server.id === serverId)?.name) || serverId || "";
  return warnings.map((warning) => {
    switch (warning.kind) {
      case "peerNotUpdated":
        return t("vibeNetworkWarnings.peerNotUpdated", { name: nameOf(warning.serverId), message: warning.message });
      case "noHandshake":
        return t("vibeNetworkWarnings.noHandshake");
      case "firewall":
        return t("vibeNetworkWarnings.firewall", { message: warning.message });
      case "dns":
        return warning.serverId
          ? t("vibeNetworkWarnings.dns", { name: nameOf(warning.serverId), message: warning.message })
          : t("vibeNetworkWarnings.dnsNotRun", { message: warning.message });
      case "bindAddresses":
        return t("vibeNetworkWarnings.bindAddresses", { message: warning.message });
    }
  });
}
