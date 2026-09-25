import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { Icon } from "@/components/ui/Icon";
import { IconButton } from "@/components/ui/IconButton";
import { SkeletonRows } from "@/components/ui/SkeletonRows";
import { useModalDialog } from "@/hooks/useModalDialog";
import { joinVibeNetwork } from "@/services/networkService";
import {
  enableServerFirewall,
  getNodeFirewallOverview,
  installDocker,
  installUfw,
  installWireguard,
  previewServerFirewallRules,
  probeServerCapabilities,
  type FirewallRule,
} from "@/services/serverService";
import { useServersStore } from "@/stores/serversStore";
import { toastSuccess } from "@/stores/toastStore";
import type { NodeCapabilities } from "@/types/server";
import { AgentPairingFlow } from "./AgentPairingFlow";
import "./AddServerModal.css";
import "./forms.css";
import { errorMessage } from "@/services/tauri";
import type { ServerModalActivity } from "@/stores/serverModalStore";

interface NodeSetupWizardProps {
  serverId: string;
  serverName: string;
  onClose: () => void;
  /** Told while an install, the firewall step or joining the network is
   *  running, so the dialog around this can move it to the background if it
   *  is closed mid-way instead of leaving nobody knowing whether it finished. */
  onActivity?: (activity: Partial<ServerModalActivity>) => void;
}

type Requirement = "docker" | "wireguard" | "ufw";

const INSTALLERS: Record<Requirement, (id: string) => Promise<NodeCapabilities>> = {
  docker: installDocker,
  wireguard: installWireguard,
  ufw: installUfw,
};

function ruleLabel(t: (key: string, opts?: Record<string, unknown>) => string, rule: FirewallRule, isFirst: boolean): string {
  const base = `${rule.port}/${rule.protocol.toUpperCase()}`;
  if (isFirst) return t("secureFirewallModal.sshRule", { rule: base });
  if (rule.sourceCidr) return t("secureFirewallModal.scopedRule", { rule: base, cidr: rule.sourceCidr });
  return base;
}

/**
 * "Add Node" setup, spun into a standalone, re-runnable flow rather than a
 * one-shot step in `AddServerModal` (design doc's own "Setup Page" - check
 * requirements, offer to install what's missing, pair the Vibe Agent,
 * configure the firewall, join the Vibe Network, verify). The requirements/
 * firewall/network sections below are SSH-mode only (an already-Agent-mode
 * Node's capabilities come from its own handshake instead) - the Agent
 * section is the one part of this wizard that's meaningful for an SSH-mode
 * Node specifically, since pairing one is exactly how it *becomes*
 * Agent-mode.
 */
export function NodeSetupWizard({ serverId, serverName, onClose, onActivity }: NodeSetupWizardProps) {
  const { t } = useTranslation();
  const backdrop = useModalDialog(onClose, { labelledBy: "nodesetupwizard-dialog-title-1" });
  const server = useServersStore((s) => s.servers.find((srv) => srv.id === serverId));
  const alreadyAgentMode = server?.connectionMode === "agent";

  const [capabilities, setCapabilities] = useState<NodeCapabilities | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [installing, setInstalling] = useState<Requirement | null>(null);
  const [installError, setInstallError] = useState<string | null>(null);
  const [firewallActive, setFirewallActive] = useState(false);
  const [firewallRules, setFirewallRules] = useState<FirewallRule[] | null>(null);
  const [firewallLoadError, setFirewallLoadError] = useState<string | null>(null);
  const [securing, setSecuring] = useState(false);
  const [securingError, setSecuringError] = useState<string | null>(null);
  const [joiningNetwork, setJoiningNetwork] = useState(false);
  const [networkJoined, setNetworkJoined] = useState(false);
  const [networkError, setNetworkError] = useState<string | null>(null);

  const working = installing !== null || securing || joiningNetwork;
  const failure = installError ?? securingError ?? networkError;
  useEffect(() => {
    onActivity?.({ busy: working, label: t("backgroundTasks.settingUp", { name: serverName }), error: failure });
  }, [working, failure, serverName, onActivity, t]);

  async function refresh() {
    setLoading(true);
    setLoadError(null);
    try {
      setCapabilities(await probeServerCapabilities(serverId));
    } catch (err) {
      setLoadError(errorMessage(err, t));
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [serverId]);

  // Shown as soon as ufw is installed - "domyślnie zabezpieczone" (secure
  // by default) without ever skipping the one safety-critical step this
  // still needs: the connecting admin sees the *exact* rule set (SSH always
  // first, always included) before anything is actually enforced. See
  // `secureFirewallModal`'s own former doc comment - now inlined here - for
  // why `enable_node_firewall` itself is deliberately never automatic: a
  // wrong rule on a remote box can lock the admin out with no way back in
  // except console access, so a silent "just turn it on" default would be
  // exactly the wrong kind of "convenient".
  useEffect(() => {
    if (!capabilities?.ufw) return;
    let cancelled = false;
    getNodeFirewallOverview(serverId)
      .then((overview) => {
        if (cancelled) return;
        setFirewallActive(overview.active);
        if (!overview.active) {
          return previewServerFirewallRules(serverId).then((rules) => {
            if (!cancelled) setFirewallRules(rules);
          });
        }
      })
      .catch((err) => {
        if (!cancelled) setFirewallLoadError(errorMessage(err, t));
      });
    return () => {
      cancelled = true;
    };
  }, [capabilities?.ufw, serverId, t]);

  async function handleSecure() {
    setSecuring(true);
    setSecuringError(null);
    try {
      const result = await enableServerFirewall(serverId);
      if (!result.backend) {
        setSecuringError(t("secureFirewallModal.noBackend"));
        return;
      }
      setFirewallActive(true);
      toastSuccess(t("secureFirewallModal.securedToast", { name: serverName, backend: result.backend }));
    } catch (err) {
      setSecuringError(errorMessage(err, t));
    } finally {
      setSecuring(false);
    }
  }

  async function handleInstall(requirement: Requirement) {
    setInstalling(requirement);
    setInstallError(null);
    try {
      setCapabilities(await INSTALLERS[requirement](serverId));
      toastSuccess(t(`nodeSetup.installedToast.${requirement}`));
    } catch (err) {
      setInstallError(errorMessage(err, t));
    } finally {
      setInstalling(null);
    }
  }

  async function handleJoinNetwork() {
    setJoiningNetwork(true);
    setNetworkError(null);
    try {
      await joinVibeNetwork(serverId);
      setNetworkJoined(true);
      toastSuccess(t("nodeSetup.joinedNetworkToast"));
    } catch (err) {
      setNetworkError(errorMessage(err, t));
    } finally {
      setJoiningNetwork(false);
    }
  }

  const rows: { key: Requirement; label: string; present: boolean }[] = capabilities
    ? [
        { key: "docker", label: t("nodeSetup.docker"), present: capabilities.docker },
        { key: "wireguard", label: t("nodeSetup.wireguard"), present: capabilities.wireguard },
        { key: "ufw", label: t("nodeSetup.ufw"), present: capabilities.ufw },
      ]
    : [];
  const allReady = Boolean(capabilities?.docker && capabilities?.wireguard && capabilities?.ufw);

  return (
    <div className="modal-backdrop" {...backdrop.backdropProps}>
      <div className="modal-panel" {...backdrop.panelProps}>
        <div className="modal-header">
          <h2 className="modal-title" id="nodesetupwizard-dialog-title-1">{t("nodeSetup.title", { name: serverName })}</h2>
          <IconButton icon="x" size="sm" onClick={onClose} title={t("common.close")} />
        </div>
        <div className="modal-body">
          <p className="dialog-body-text">{t("nodeSetup.intro")}</p>

          <h3 className="card-title">{t("nodeSetup.requirementsTitle")}</h3>
          {loadError && <p className="form-note form-note-danger form-note-spaced">{loadError}</p>}
          {loading ? (
            <SkeletonRows />
          ) : (
            <ul className="server-list">
              {rows.map((row) => (
                <li key={row.key} className="server-list-item">
                  <Icon name={row.present ? "check" : "x"} size={16} />
                  <div className="server-list-main">
                    <span className="server-list-name">{row.label}</span>
                    <span className="server-list-host">{row.present ? t("nodeSetup.present") : t("nodeSetup.missing")}</span>
                  </div>
                  {!row.present && (
                    <Button size="sm" variant="secondary" onClick={() => handleInstall(row.key)} disabled={installing !== null}>
                      {installing === row.key ? t("common.loading") : t("nodeSetup.installAuto")}
                    </Button>
                  )}
                </li>
              ))}
            </ul>
          )}
          {installError && <p className="form-note form-note-danger form-note-spaced">{installError}</p>}
          <div className="form-actions">
            <Button variant="secondary" size="sm" onClick={refresh} disabled={loading || installing !== null}>
              <Icon name="refresh-cw" size={14} />
              {t("nodeSetup.recheck")}
            </Button>
          </div>

          <h3 className="card-title">{t("nodeSetup.agentTitle")}</h3>
          {alreadyAgentMode ? (
            <p className="form-note form-note-success">
              <Icon name="check" size={14} /> {t("nodeSetup.agentAlreadyPaired")}
            </p>
          ) : (
            <>
              <p className="form-note">{t("nodeSetup.agentNote")}</p>
              <AgentPairingFlow onPaired={() => {}} upgradeExistingServer={{ id: serverId, host: server?.host ?? "" }} />
            </>
          )}

          <h3 className="card-title">{t("nodeSetup.firewallTitle")}</h3>
          <p className="form-note">{t("nodeSetup.firewallNote")}</p>
          {!capabilities?.ufw ? (
            <p className="form-note">{t("nodeSetup.ufwRequiredHint")}</p>
          ) : firewallActive ? (
            <p className="form-note form-note-success">
              <Icon name="check" size={14} /> {t("secureFirewallModal.alreadyActive")}
            </p>
          ) : (
            <>
              {firewallLoadError && <p className="form-note form-note-danger form-note-spaced">{firewallLoadError}</p>}
              {securingError && <p className="form-note form-note-danger form-note-spaced">{securingError}</p>}
              {firewallRules && (
                <ul className="secure-firewall-rule-list">
                  {firewallRules.map((rule, i) => (
                    <li key={`${rule.port}-${rule.protocol}-${rule.sourceCidr ?? ""}`}>
                      <Icon name="shield" size={14} />
                      {ruleLabel(t, rule, i === 0)}
                    </li>
                  ))}
                </ul>
              )}
              <div className="form-actions">
                <Button size="sm" onClick={handleSecure} disabled={securing || !firewallRules}>
                  <Icon name="lock" size={14} />
                  {securing ? t("common.loading") : t("nodeSetup.configureFirewall")}
                </Button>
              </div>
            </>
          )}

          <h3 className="card-title">{t("nodeSetup.networkTitle")}</h3>
          <p className="form-note">{t("nodeSetup.networkNote")}</p>
          {networkError && <p className="form-note form-note-danger form-note-spaced">{networkError}</p>}
          <div className="form-actions">
            <Button
              variant="secondary"
              size="sm"
              onClick={handleJoinNetwork}
              disabled={!capabilities?.wireguard || joiningNetwork || networkJoined}
              title={!capabilities?.wireguard ? t("nodeSetup.wireguardRequiredHint") : undefined}
            >
              <Icon name="zap" size={14} />
              {networkJoined ? t("nodeSetup.joinedNetwork") : joiningNetwork ? t("common.loading") : t("nodeSetup.joinNetwork")}
            </Button>
          </div>

          <div className="form-actions form-actions-split">
            <p className="form-note">{allReady ? t("nodeSetup.allReady") : t("nodeSetup.someOutstanding")}</p>
            <Button onClick={onClose}>{t("common.done")}</Button>
          </div>
        </div>
      </div>
    </div>
  );
}
