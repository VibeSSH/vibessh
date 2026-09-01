import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Badge } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { EmptyState } from "@/components/ui/EmptyState";
import { HostAddress } from "@/components/ui/HostAddress";
import { Icon } from "@/components/ui/Icon";
import { IconButton } from "@/components/ui/IconButton";
import { OverflowMenu } from "@/components/ui/OverflowMenu";
import { RowPicker, serverRowPickerOption, type RowPickerOption } from "@/components/ui/RowPicker";
import { SkeletonRows } from "@/components/ui/SkeletonRows";
import { Switch } from "@/components/ui/Switch";
import { useModalDialog } from "@/hooks/useModalDialog";
import { useServerPinging } from "@/hooks/useServerPinging";
import { addApplicationPort, listApplications, removeApplicationPort, updateApplicationPort } from "@/services/applicationService";
import {
  createDnsAlias,
  deleteDnsAlias,
  getVibeNetworkStatus,
  joinVibeNetwork,
  leaveVibeNetwork,
  listDnsRecords,
  listNetworkMembers,
  listNodeEndpoints,
  resolveDnsView,
  syncVibeDns,
  syncVibeNetwork,
  updateDnsAlias,
  verifyDnsAlias,
} from "@/services/networkService";
import { usePingStore } from "@/stores/pingStore";
import { useServersStore, type ManagedServer } from "@/stores/serversStore";
import { toastError, toastSuccess } from "@/stores/toastStore";
import type { Application, PortInput } from "@/types/application";
import { formatRelativeTime } from "@/utils/formatRelativeTime";
import type { DnsRecord, DnsView, NodeEndpoint, NodeMeshStatus, NodeNetworkMember, VibeNetworkSyncResult } from "@/types/network";
import "@/components/servers/AddServerModal.css";
import "@/components/servers/forms.css";
import "@/components/servers/ServerCard.css";
import "./ApplicationDetail.css";
import "./Servers.css";
import "./pages.css";
import "./VibeNetwork.css";
import { errorMessage } from "@/services/tauri";

type Tab = "nodes" | "endpoints" | "dns";

export function VibeNetwork() {
  const { t } = useTranslation();
  const servers = useServersStore((s) => s.servers);
  useServerPinging(servers);

  const [tab, setTab] = useState<Tab>("nodes");
  const [members, setMembers] = useState<NodeNetworkMember[]>([]);
  const [meshStatus, setMeshStatus] = useState<NodeMeshStatus[]>([]);
  const [dnsView, setDnsView] = useState<DnsView[]>([]);
  const [applications, setApplications] = useState<Application[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const [addNodeOpen, setAddNodeOpen] = useState(false);
  const [syncing, setSyncing] = useState(false);
  const [syncResults, setSyncResults] = useState<VibeNetworkSyncResult[] | null>(null);
  const [advanced, setAdvanced] = useState(false);

  const [leavingMember, setLeavingMember] = useState<NodeNetworkMember | null>(null);
  const [leaveBusy, setLeaveBusy] = useState(false);
  const [leaveError, setLeaveError] = useState<string | null>(null);
  const leaveBackdrop = useModalDialog(() => !leaveBusy && setLeavingMember(null), { labelledBy: "vibenetwork-dialog-title-1" });

  const reload = useCallback(() => {
    setLoading(true);
    setError(null);
    Promise.all([listNetworkMembers(), getVibeNetworkStatus(), resolveDnsView(), listApplications()])
      .then(([m, s, d, apps]) => {
        setMembers(m);
        setMeshStatus(s);
        setDnsView(d);
        setApplications(apps);
      })
      .catch((err) => setError(errorMessage(err, t)))
      .finally(() => setLoading(false));
  }, [t]);

  useEffect(reload, [reload]);

  async function handleSync() {
    setSyncing(true);
    setError(null);
    try {
      const results = await syncVibeNetwork();
      setSyncResults(results);
      for (const result of results) {
        if (result.ok) continue;
        const detail = [result.meshError, result.firewallError, result.dnsError].filter(Boolean).join(" · ");
        toastError(t("vibeNetwork.syncErrorToast", { name: serverName(result.serverId), detail }));
      }
      reload();
    } catch (err) {
      setError(errorMessage(err, t));
    } finally {
      setSyncing(false);
    }
  }

  async function handleConfirmLeave() {
    if (!leavingMember) return;
    setLeaveBusy(true);
    setLeaveError(null);
    try {
      await leaveVibeNetwork(leavingMember.serverId);
      toastSuccess(t("vibeNetwork.leftToast", { name: serverName(leavingMember.serverId) }));
      setLeavingMember(null);
      reload();
    } catch (err) {
      setLeaveError(errorMessage(err, t));
    } finally {
      setLeaveBusy(false);
    }
  }

  function serverName(serverId: string): string {
    return servers.find((s) => s.id === serverId)?.name ?? serverId;
  }

  const joinableServers = useMemo(() => servers.filter((s) => s.connectionMode === "ssh" && !members.some((m) => m.serverId === s.id)), [servers, members]);

  const networkHealthy = meshStatus.length > 0 && meshStatus.every((s) => s.reachable);

  return (
    <div className="page">
      <div className="page-header page-header-row">
        <div>
          <h1 className="page-title">{t("vibeNetwork.title")}</h1>
          <p className="page-subtitle">{t("vibeNetwork.subtitle")}</p>
        </div>
        <div className="vibe-network-header-actions">
          <Switch checked={advanced} onChange={setAdvanced} label={t("vibeNetwork.advancedToggle")} />
          <Button variant="secondary" onClick={handleSync} disabled={syncing || members.length === 0}>
            <Icon name="refresh-cw" size={16} />
            {syncing ? t("vibeNetwork.syncing") : t("vibeNetwork.syncButton")}
          </Button>
          <Button onClick={() => setAddNodeOpen(true)}>
            <Icon name="plus" size={16} />
            {t("vibeNetwork.addNode")}
          </Button>
        </div>
      </div>

      {members.length > 0 && (
        <div className="vibe-network-status-row">
          <Badge tone={networkHealthy ? "success" : "danger"}>{networkHealthy ? t("vibeNetwork.networkHealthy") : t("vibeNetwork.networkDegraded")}</Badge>
          <span className="form-note">{t("vibeNetwork.memberCount", { count: members.length })}</span>
        </div>
      )}

      {error && <p className="page-error-note">{error}</p>}

      {syncResults && (
        <Card title={t("vibeNetwork.syncResultsTitle")}>
          <ul className="vibe-network-sync-results">
            {syncResults.map((result) => (
              <li key={result.serverId} className="vibe-network-sync-result-row">
                <span>{serverName(result.serverId)}</span>
                <Badge tone={result.ok ? "success" : "danger"}>{result.ok ? t("vibeNetwork.syncOk") : t("vibeNetwork.syncOutOfSync")}</Badge>
                {!result.ok && (
                  <span className="form-note form-note-danger">
                    {[result.meshError, result.firewallError, result.dnsError].filter(Boolean).join(" · ")}
                  </span>
                )}
              </li>
            ))}
          </ul>
        </Card>
      )}

      <div className="page-tabs">
        <button className={`modal-tab ${tab === "nodes" ? "modal-tab-active" : ""}`} onClick={() => setTab("nodes")}>
          {t("vibeNetwork.tabNodes")}
        </button>
        <button className={`modal-tab ${tab === "endpoints" ? "modal-tab-active" : ""}`} onClick={() => setTab("endpoints")}>
          {t("vibeNetwork.tabEndpoints")}
        </button>
        <button className={`modal-tab ${tab === "dns" ? "modal-tab-active" : ""}`} onClick={() => setTab("dns")}>
          {t("vibeNetwork.tabDns")}
        </button>
      </div>

      {loading ? (
        <SkeletonRows />
      ) : (
        <>
          {tab === "nodes" &&
            (members.length === 0 ? (
              <Card>
                <EmptyState icon="wifi" title={t("vibeNetwork.emptyTitle")} description={t("vibeNetwork.emptyDescription")} />
              </Card>
            ) : (
              <div className="vibe-network-node-grid">
                {members.map((member) => (
                  <NodeCard
                    key={member.serverId}
                    member={member}
                    name={serverName(member.serverId)}
                    status={meshStatus.find((s) => s.serverId === member.serverId) ?? null}
                    dnsName={dnsView.find((v) => v.kind.type === "node" && v.serverId === member.serverId)?.hostname ?? null}
                    serverName={serverName}
                    advanced={advanced}
                    applicationCount={applications.filter((a) => a.serverId === member.serverId).length}
                    onLeave={() => {
                      setLeaveError(null);
                      setLeavingMember(member);
                    }}
                  />
                ))}
              </div>
            ))}

          {tab === "endpoints" && <EndpointsPanel members={members} servers={servers} meshStatus={meshStatus} advanced={advanced} />}

          {tab === "dns" && <DnsPanel members={members} dnsView={dnsView} onChanged={reload} />}
        </>
      )}

      {addNodeOpen && (
        <AddNodeModal
          joinableServers={joinableServers}
          onClose={() => setAddNodeOpen(false)}
          onJoined={() => {
            setAddNodeOpen(false);
            reload();
          }}
        />
      )}

      {leavingMember && (
        <div className="modal-backdrop" {...leaveBackdrop.backdropProps}>
          <div className="modal-panel modal-panel-sm" {...leaveBackdrop.panelProps}>
            <div className="modal-header">
              <h2 className="modal-title" id="vibenetwork-dialog-title-1">{t("vibeNetwork.leaveTitle")}</h2>
              <IconButton icon="x" size="sm" onClick={() => setLeavingMember(null)} title={t("common.close")} />
            </div>
            <div className="modal-body">
              <p className="dialog-body-text">{t("vibeNetwork.leaveBody", { name: serverName(leavingMember.serverId) })}</p>
              {leaveError && <p className="form-note form-note-danger form-note-spaced">{leaveError}</p>}
              <div className="form-actions">
                <Button variant="secondary" onClick={() => setLeavingMember(null)} disabled={leaveBusy}>
                  {t("common.cancel")}
                </Button>
                <Button variant="danger" onClick={handleConfirmLeave} disabled={leaveBusy}>
                  {leaveBusy ? t("common.saving") : t("vibeNetwork.leaveConfirm")}
                </Button>
              </div>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

interface NodeCardProps {
  member: NodeNetworkMember;
  name: string;
  status: NodeMeshStatus | null;
  dnsName: string | null;
  serverName: (serverId: string) => string;
  advanced: boolean;
  applicationCount: number;
  onLeave: () => void;
}

function NodeCard({ member, name, status, dnsName, serverName, advanced, applicationCount, onLeave }: NodeCardProps) {
  const { t } = useTranslation();
  const nowSeconds = Date.now() / 1000;
  const reachable = status?.reachable ?? false;
  const peers = status?.peers ?? [];
  const latencyMs = usePingStore((s) => s.latencies[member.serverId]);
  const lastSyncUnix = peers.length > 0 ? Math.max(...peers.map((p) => p.latestHandshakeUnix)) : 0;

  const [endpointCount, setEndpointCount] = useState<number | null>(null);
  useEffect(() => {
    let cancelled = false;
    listNodeEndpoints(member.serverId)
      .then((eps) => !cancelled && setEndpointCount(eps.length))
      .catch(() => !cancelled && setEndpointCount(null));
    return () => {
      cancelled = true;
    };
  }, [member.serverId]);

  return (
    <Card>
      <div className="vibe-network-node-header">
        <div className="server-card-avatar glossy-tile">
          <Icon name="server" size={16} />
        </div>
        <div className="vibe-network-node-title">
          <p className="server-card-name">{name}</p>
          <HostAddress value={member.wireguardIp} className="server-card-host" />
        </div>
        <Badge tone={reachable ? "success" : "danger"}>{reachable ? t("vibeNetwork.online") : t("vibeNetwork.offline")}</Badge>
        <OverflowMenu
          ariaLabel={t("vibeNetwork.nodeMenuAria", { name })}
          items={[{ label: t("vibeNetwork.leaveButton"), icon: "trash", danger: true, onClick: onLeave }]}
        />
      </div>

      <div className="vibe-network-node-facts">
        {dnsName && (
          <div className="vibe-network-fact">
            <span className="form-label">{t("vibeNetwork.dnsName")}</span>
            <span className="vibe-network-fact-value">{dnsName}</span>
          </div>
        )}
        <div className="vibe-network-fact">
          <span className="form-label">{t("vibeNetwork.connectionLabel")}</span>
          <span className="vibe-network-fact-value">{reachable ? t("vibeNetwork.connectionActive") : t("vibeNetwork.connectionInactive")}</span>
        </div>
        {typeof latencyMs === "number" && (
          <div className="vibe-network-fact">
            <span className="form-label">{t("vibeNetwork.latencyLabel")}</span>
            <span className="vibe-network-fact-value">{latencyMs} ms</span>
          </div>
        )}
        <div className="vibe-network-fact">
          <span className="form-label">{t("vibeNetwork.applicationsLabel")}</span>
          <span className="vibe-network-fact-value">{applicationCount}</span>
        </div>
        <div className="vibe-network-fact">
          <span className="form-label">{t("vibeNetwork.endpointsLabel")}</span>
          <span className="vibe-network-fact-value">{endpointCount ?? "—"}</span>
        </div>
        <div className="vibe-network-fact">
          <span className="form-label">{t("vibeNetwork.lastSyncLabel")}</span>
          <span className="vibe-network-fact-value">{lastSyncUnix > 0 ? formatRelativeTime(lastSyncUnix * 1000, t) : t("vibeNetwork.neverHandshaked")}</span>
        </div>
      </div>

      {advanced && (
        <div className="vibe-network-advanced">
          <div className="vibe-network-fact">
            <span className="form-label">{t("vibeNetwork.publicKey")}</span>
            <span className="vibe-network-fact-value vibe-network-mono" title={member.wireguardPublicKey}>
              {member.wireguardPublicKey}
            </span>
          </div>
          <div className="vibe-network-fact">
            <span className="form-label">{t("vibeNetwork.interface")}</span>
            <span className="vibe-network-fact-value vibe-network-mono">wg-vibessh0</span>
          </div>
          {peers.map((peer) => (
            <div className="vibe-network-fact" key={peer.serverId}>
              <span className="form-label">{serverName(peer.serverId)}</span>
              <span className="vibe-network-fact-value">
                {peer.latestHandshakeUnix === 0
                  ? t("vibeNetwork.neverHandshaked")
                  : t("vibeNetwork.handshakeAgo", { seconds: Math.max(0, Math.round(nowSeconds - peer.latestHandshakeUnix)) })}
                {" · "}
                {t("vibeNetwork.transfer", { rx: formatBytes(peer.rxBytes), tx: formatBytes(peer.txBytes) })}
              </span>
            </div>
          ))}
        </div>
      )}
    </Card>
  );
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KiB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MiB`;
  return `${(bytes / 1024 / 1024 / 1024).toFixed(1)} GiB`;
}

interface AddNodeModalProps {
  joinableServers: ManagedServer[];
  onClose: () => void;
  onJoined: () => void;
}

/** The whole "Add Node" flow the spec asks for: pick an existing Server,
 * click "Dołącz do Vibe Network" - VibeSSH generates the WireGuard keys,
 * allocates the private IP, syncs every peer's config, and reconciles the
 * firewall, all server-side (`services::network_service::join_node`). No
 * CIDR, peer, or firewall rule is ever typed here. */
function AddNodeModal({ joinableServers, onClose, onJoined }: AddNodeModalProps) {
  const { t } = useTranslation();
  const backdrop = useModalDialog(onClose, { labelledBy: "vibenetwork-dialog-title-2" });
  const [serverId, setServerId] = useState(joinableServers[0]?.id ?? "");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [step, setStep] = useState<"idle" | "joining" | "done">("idle");

  async function handleJoin() {
    if (!serverId) return;
    setBusy(true);
    setError(null);
    setStep("joining");
    try {
      await joinVibeNetwork(serverId);
      setStep("done");
      toastSuccess(t("vibeNetwork.joinedToast"));
      onJoined();
    } catch (err) {
      setStep("idle");
      setError(errorMessage(err, t));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="modal-backdrop" {...backdrop.backdropProps}>
      <div className="modal-panel modal-panel-sm" {...backdrop.panelProps}>
        <div className="modal-header">
          <h2 className="modal-title" id="vibenetwork-dialog-title-2">{t("vibeNetwork.addNodeTitle")}</h2>
          <IconButton icon="x" size="sm" onClick={onClose} title={t("common.close")} />
        </div>
        <div className="modal-body">
          {joinableServers.length === 0 ? (
            <p className="form-note">{t("vibeNetwork.noJoinableServers")}</p>
          ) : (
            <RowPicker
              label={t("vibeNetwork.chooseServer")}
              placeholder={t("vibeNetwork.chooseServer")}
              value={serverId}
              onChange={setServerId}
              disabled={busy}
              options={joinableServers.map((s) => serverRowPickerOption(s, t))}
            />
          )}
          <p className="form-note">{t("vibeNetwork.addNodeHelp")}</p>
          {step === "joining" && <p className="form-note">{t("vibeNetwork.joining")}</p>}
          {error && <p className="form-note form-note-danger form-note-spaced">{error}</p>}
          <div className="form-actions">
            <Button variant="secondary" onClick={onClose} disabled={busy}>
              {t("common.cancel")}
            </Button>
            <Button onClick={handleJoin} disabled={busy || !serverId}>
              {busy ? t("vibeNetwork.joining") : t("vibeNetwork.joinButton")}
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}

interface EndpointsPanelProps {
  members: NodeNetworkMember[];
  servers: ManagedServer[];
  meshStatus: NodeMeshStatus[];
  advanced: boolean;
}

/** "Endpoints" - a Node-scoped read over the *existing* Application Ports
 * data (see the Rust `NodeEndpoint`'s own doc comment for why this isn't a
 * separate model) - CRUD reuses the exact same commands the Ports tab
 * already uses. */
function EndpointsPanel({ members, servers, meshStatus, advanced }: EndpointsPanelProps) {
  const { t } = useTranslation();
  const nodeOptions: RowPickerOption[] = members.map((m) => {
    const server = servers.find((s) => s.id === m.serverId);
    const reachable = meshStatus.find((s) => s.serverId === m.serverId)?.reachable ?? false;
    return {
      id: m.serverId,
      name: server?.name ?? m.serverId,
      meta: m.wireguardIp,
      status: { tone: reachable ? "success" : "danger", label: t(reachable ? "vibeNetwork.online" : "vibeNetwork.offline") },
    };
  });
  const [serverId, setServerId] = useState(members[0]?.serverId ?? "");
  const [endpoints, setEndpoints] = useState<NodeEndpoint[]>([]);
  const [applications, setApplications] = useState<Application[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [formOpen, setFormOpen] = useState(false);
  const [editing, setEditing] = useState<NodeEndpoint | null>(null);

  const reload = useCallback(() => {
    if (!serverId) return;
    setLoading(true);
    setError(null);
    Promise.all([listNodeEndpoints(serverId), listApplications()])
      .then(([eps, apps]) => {
        setEndpoints(eps);
        setApplications(apps.filter((a) => a.serverId === serverId));
      })
      .catch((err) => setError(errorMessage(err, t)))
      .finally(() => setLoading(false));
  }, [serverId, t]);

  useEffect(reload, [reload]);

  useEffect(() => {
    if (!serverId && members.length > 0) setServerId(members[0].serverId);
  }, [members, serverId]);

  async function handleDelete(endpoint: NodeEndpoint) {
    try {
      await removeApplicationPort(endpoint.applicationId, endpoint.id);
      reload();
    } catch (err) {
      setError(errorMessage(err, t));
    }
  }

  if (members.length === 0) {
    return (
      <Card>
        <EmptyState icon="wifi" title={t("vibeNetwork.emptyTitle")} description={t("vibeNetwork.emptyDescription")} />
      </Card>
    );
  }

  return (
    <>
      <div className="application-detail-header-row">
        <div className="vibe-network-node-picker">
          <RowPicker label={t("vibeNetwork.chooseServer")} placeholder={t("vibeNetwork.chooseServer")} value={serverId} onChange={setServerId} options={nodeOptions} />
        </div>
        <Button
          size="sm"
          onClick={() => {
            setEditing(null);
            setFormOpen(true);
          }}
          disabled={applications.length === 0}
        >
          <Icon name="plus" size={14} />
          {t("vibeNetwork.addEndpoint")}
        </Button>
      </div>
      {applications.length === 0 && <p className="form-note">{t("vibeNetwork.noApplicationsOnNode")}</p>}
      {error && <p className="page-error-note">{error}</p>}

      <Card>
        {loading ? (
          <SkeletonRows />
        ) : endpoints.length === 0 ? (
          <EmptyState icon="wifi" title={t("portsTab.emptyTitle")} description={t("portsTab.emptyDescription")} />
        ) : (
          <ul className="server-list">
            {endpoints.map((endpoint) => (
              <li key={endpoint.id} className="server-list-item">
                <div className="server-list-main">
                  <span className="server-list-name" title={endpoint.name}>
                    {endpoint.name}
                  </span>
                  <span className="server-list-host">
                    {endpoint.internalPort}/{endpoint.protocol.toUpperCase()} · {t("vibeNetwork.pointsTo", { application: endpoint.applicationName, port: endpoint.internalPort })}
                  </span>
                  {advanced && (
                    <span className="server-list-host vibe-network-mono">
                      {endpoint.bindAddress}:{endpoint.externalPort ?? endpoint.internalPort}
                      {endpoint.externalPort ? ` → ${endpoint.internalPort}` : ""}
                    </span>
                  )}
                </div>
                <Badge tone="neutral">{visibilityLabel(endpoint.visibility, t)}</Badge>
                <IconButton
                  icon="edit"
                  size="sm"
                  title={t("portsTab.editAria", { name: endpoint.name })}
                  onClick={() => {
                    setEditing(endpoint);
                    setFormOpen(true);
                  }}
                />
                {!endpoint.required && (
                  <IconButton icon="trash" size="sm" danger title={t("portsTab.deleteAria", { name: endpoint.name })} onClick={() => handleDelete(endpoint)} />
                )}
              </li>
            ))}
          </ul>
        )}
      </Card>

      {formOpen && (
        <EndpointFormModal
          applications={applications}
          editing={editing}
          onClose={() => setFormOpen(false)}
          onSaved={() => {
            setFormOpen(false);
            reload();
          }}
        />
      )}
    </>
  );
}

function visibilityLabel(visibility: NodeEndpoint["visibility"], t: (key: string) => string): string {
  return t(`applicationNetwork.visibility.${visibility}`);
}

interface EndpointFormModalProps {
  applications: Application[];
  editing: NodeEndpoint | null;
  onClose: () => void;
  onSaved: () => void;
}

function EndpointFormModal({ applications, editing, onClose, onSaved }: EndpointFormModalProps) {
  const { t } = useTranslation();
  const backdrop = useModalDialog(onClose, { labelledBy: "vibenetwork-dialog-title-3" });
  const [applicationId, setApplicationId] = useState(editing?.applicationId ?? applications[0]?.id ?? "");
  const [name, setName] = useState(editing?.name ?? "");
  const [protocol, setProtocol] = useState<"tcp" | "udp">(editing?.protocol ?? "tcp");
  const [internalPort, setInternalPort] = useState(editing ? String(editing.internalPort) : "");
  const [externalPort, setExternalPort] = useState(editing?.externalPort ? String(editing.externalPort) : "");
  const [visibility, setVisibility] = useState<NodeEndpoint["visibility"]>(editing?.visibility ?? "public");
  const [customAddress, setCustomAddress] = useState(editing?.bindAddress ?? "");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    const internal = Number(internalPort);
    if (!name.trim() || !applicationId || !Number.isInteger(internal) || internal < 1 || internal > 65535) {
      setError(t("portsTab.invalidForm"));
      return;
    }
    const input: PortInput = {
      name: name.trim(),
      protocol,
      bindAddress: visibility === "custom" ? customAddress.trim() : "0.0.0.0",
      internalPort: internal,
      externalPort: externalPort.trim() ? Number(externalPort) : undefined,
      visibility,
    };

    setBusy(true);
    setError(null);
    try {
      if (editing) {
        await updateApplicationPort(editing.applicationId, editing.id, input);
      } else {
        await addApplicationPort(applicationId, input);
      }
      onSaved();
    } catch (err) {
      setError(errorMessage(err, t));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="modal-backdrop" {...backdrop.backdropProps}>
      <div className="modal-panel" {...backdrop.panelProps}>
        <div className="modal-header">
          <h2 className="modal-title" id="vibenetwork-dialog-title-3">{editing ? t("portsTab.editTitle") : t("vibeNetwork.addEndpoint")}</h2>
          <IconButton icon="x" size="sm" onClick={onClose} title={t("common.close")} />
        </div>
        <form className="server-form" onSubmit={handleSubmit}>
          <div className="modal-body">
            {error && <p className="form-note form-note-danger form-note-spaced">{error}</p>}
            {!editing && (
              <label className="form-field">
                <span className="form-label">{t("vibeNetwork.chooseApplication")}</span>
                <select className="form-input" value={applicationId} onChange={(e) => setApplicationId(e.target.value)}>
                  {applications.map((a) => (
                    <option key={a.id} value={a.id}>
                      {a.name}
                    </option>
                  ))}
                </select>
              </label>
            )}
            <label className="form-field">
              <span className="form-label">{t("portsTab.name")}</span>
              <input className="form-input" value={name} onChange={(e) => setName(e.target.value)} placeholder={t("portsTab.namePlaceholder")} />
            </label>
            <div className="form-row">
              <label className="form-field form-field-narrow">
                <span className="form-label">{t("portsTab.protocol")}</span>
                <select className="form-input" value={protocol} onChange={(e) => setProtocol(e.target.value as "tcp" | "udp")}>
                  <option value="tcp">TCP</option>
                  <option value="udp">UDP</option>
                </select>
              </label>
              <label className="form-field">
                <span className="form-label">{t("portsTab.internalPort")}</span>
                <input className="form-input" type="number" min={1} max={65535} value={internalPort} onChange={(e) => setInternalPort(e.target.value)} placeholder="25565" />
              </label>
              <label className="form-field">
                <span className="form-label">{t("portsTab.externalPort")}</span>
                <input
                  className="form-input"
                  type="number"
                  min={1}
                  max={65535}
                  value={externalPort}
                  onChange={(e) => setExternalPort(e.target.value)}
                  placeholder={t("portsTab.externalPortPlaceholder")}
                />
              </label>
            </div>
            <label className="form-field">
              <span className="form-label">{t("applicationNetwork.access")}</span>
              <select className="form-input" value={visibility} onChange={(e) => setVisibility(e.target.value as NodeEndpoint["visibility"])}>
                <option value="public">{t("applicationNetwork.visibility.public")}</option>
                <option value="vibeNetwork">{t("applicationNetwork.visibility.vibeNetwork")}</option>
                <option value="localhost">{t("applicationNetwork.visibility.localhost")}</option>
                <option value="custom">{t("applicationNetwork.visibility.custom")}</option>
              </select>
              <p className="form-note">{t(`applicationNetwork.visibilityHelp.${visibility}`)}</p>
            </label>
            {visibility === "custom" && (
              <label className="form-field">
                <span className="form-label">{t("portsTab.bindAddress")}</span>
                <input className="form-input" value={customAddress} onChange={(e) => setCustomAddress(e.target.value)} placeholder="0.0.0.0" />
              </label>
            )}
            <div className="form-actions">
              <Button type="button" variant="secondary" onClick={onClose} disabled={busy}>
                {t("common.cancel")}
              </Button>
              <Button type="submit" disabled={busy}>
                {busy ? t("common.saving") : t("common.save")}
              </Button>
            </div>
          </div>
        </form>
      </div>
    </div>
  );
}

interface DnsPanelProps {
  members: NodeNetworkMember[];
  dnsView: DnsView[];
  onChanged: () => void;
}

/** Private DNS - Node aliases are always-present and computed
 * (`hetzner-01.vibe`); service aliases are user-created here, one per
 * Application, and follow the Application if it ever moves to another
 * Node. */
function DnsPanel({ members, dnsView, onChanged }: DnsPanelProps) {
  const { t } = useTranslation();
  const [records, setRecords] = useState<DnsRecord[]>([]);
  const [applications, setApplications] = useState<Application[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [formOpen, setFormOpen] = useState(false);
  const [editing, setEditing] = useState<DnsRecord | null>(null);
  const [syncing, setSyncing] = useState(false);
  const [syncNote, setSyncNote] = useState<string | null>(null);
  const [verifying, setVerifying] = useState<string | null>(null);
  const [verifyResult, setVerifyResult] = useState<Record<string, boolean>>({});

  const reload = useCallback(() => {
    setLoading(true);
    setError(null);
    Promise.all([listDnsRecords(), listApplications()])
      .then(([r, a]) => {
        setRecords(r);
        setApplications(a);
      })
      .catch((err) => setError(errorMessage(err, t)))
      .finally(() => setLoading(false));
  }, [t]);

  useEffect(reload, [reload]);

  async function handleSync() {
    setSyncing(true);
    setSyncNote(null);
    try {
      const results = await syncVibeDns();
      const failed = results.filter((r) => !r.ok);
      setSyncNote(failed.length === 0 ? t("vibeNetwork.dnsSyncOk", { count: results.length }) : t("vibeNetwork.dnsSyncPartial", { failed: failed.length, total: results.length }));
      onChanged();
    } catch (err) {
      setSyncNote(errorMessage(err, t));
    } finally {
      setSyncing(false);
    }
  }

  async function handleVerify(record: DnsRecord) {
    const view = dnsView.find((v) => v.hostname === record.hostname);
    if (!view) return;
    setVerifying(record.id);
    try {
      const ok = await verifyDnsAlias(record.hostname, view.ip);
      setVerifyResult((prev) => ({ ...prev, [record.id]: ok }));
    } catch {
      setVerifyResult((prev) => ({ ...prev, [record.id]: false }));
    } finally {
      setVerifying(null);
    }
  }

  async function handleDelete(record: DnsRecord) {
    try {
      const results = await deleteDnsAlias(record.id);
      const failed = results.filter((r) => !r.ok);
      setSyncNote(failed.length === 0 ? t("vibeNetwork.dnsSyncOk", { count: results.length }) : t("vibeNetwork.dnsSyncPartial", { failed: failed.length, total: results.length }));
      reload();
    } catch (err) {
      setError(errorMessage(err, t));
    }
  }

  const applicationsWithoutAlias = applications.filter((a) => !records.some((r) => r.applicationId === a.id));

  return (
    <>
      <div className="application-detail-header-row">
        <p className="form-note">{t("vibeNetwork.dnsDescription")}</p>
        <div className="vibe-network-header-actions">
          <Button variant="secondary" size="sm" onClick={handleSync} disabled={syncing || members.length === 0}>
            <Icon name="refresh-cw" size={14} />
            {syncing ? t("vibeNetwork.syncing") : t("vibeNetwork.dnsSyncButton")}
          </Button>
          <Button
            size="sm"
            onClick={() => {
              setEditing(null);
              setFormOpen(true);
            }}
            disabled={applicationsWithoutAlias.length === 0}
          >
            <Icon name="plus" size={14} />
            {t("vibeNetwork.addAlias")}
          </Button>
        </div>
      </div>
      {syncNote && <p className="form-note">{syncNote}</p>}
      {error && <p className="page-error-note">{error}</p>}

      <Card>
        {loading ? (
          <SkeletonRows />
        ) : records.length === 0 ? (
          <EmptyState icon="wifi" title={t("vibeNetwork.dnsEmptyTitle")} description={t("vibeNetwork.dnsEmptyDescription")} />
        ) : (
          <ul className="server-list">
            {records.map((record) => {
              const view = dnsView.find((v) => v.hostname === record.hostname);
              const application = applications.find((a) => a.id === record.applicationId);
              const verified = verifyResult[record.id];
              return (
                <li key={record.id} className="server-list-item">
                  <div className="server-list-main">
                    <span className="server-list-name" title={record.hostname}>
                      {record.hostname}
                    </span>
                    <span className="server-list-host">
                      {application?.name ?? record.applicationId} {view ? `· ${view.ip}` : ""}
                    </span>
                  </div>
                  {verified !== undefined && <Badge tone={verified ? "success" : "danger"}>{verified ? t("vibeNetwork.verified") : t("vibeNetwork.verifyFailed")}</Badge>}
                  <IconButton
                    icon="refresh-cw"
                    size="sm"
                    title={t("vibeNetwork.verifyAria", { name: record.hostname })}
                    onClick={() => handleVerify(record)}
                    disabled={verifying === record.id || !view}
                  />
                  <IconButton
                    icon="edit"
                    size="sm"
                    title={t("portsTab.editAria", { name: record.hostname })}
                    onClick={() => {
                      setEditing(record);
                      setFormOpen(true);
                    }}
                  />
                  <IconButton icon="trash" size="sm" danger title={t("portsTab.deleteAria", { name: record.hostname })} onClick={() => handleDelete(record)} />
                </li>
              );
            })}
          </ul>
        )}
      </Card>

      {formOpen && (
        <DnsFormModal
          applications={editing ? applications : applicationsWithoutAlias}
          editing={editing}
          onClose={() => setFormOpen(false)}
          onSaved={() => {
            setFormOpen(false);
            reload();
          }}
        />
      )}
    </>
  );
}

interface DnsFormModalProps {
  applications: Application[];
  editing: DnsRecord | null;
  onClose: () => void;
  onSaved: () => void;
}

function DnsFormModal({ applications, editing, onClose, onSaved }: DnsFormModalProps) {
  const { t } = useTranslation();
  const backdrop = useModalDialog(onClose, { labelledBy: "vibenetwork-dialog-title-4" });
  const [applicationId, setApplicationId] = useState(editing?.applicationId ?? applications[0]?.id ?? "");
  const [hostname, setHostname] = useState(editing?.hostname.replace(/\.vibe$/, "") ?? "");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (!hostname.trim() || (!editing && !applicationId)) {
      setError(t("vibeNetwork.dnsInvalidForm"));
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const { syncResults } = editing ? await updateDnsAlias(editing.id, hostname.trim()) : await createDnsAlias(applicationId, hostname.trim());
      const failed = syncResults.filter((r) => !r.ok);
      if (failed.length === 0) {
        toastSuccess(t("vibeNetwork.dnsSyncOk", { count: syncResults.length }));
      } else {
        toastError(t("vibeNetwork.dnsSyncPartial", { failed: failed.length, total: syncResults.length }));
      }
      onSaved();
    } catch (err) {
      setError(errorMessage(err, t));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="modal-backdrop" {...backdrop.backdropProps}>
      <div className="modal-panel modal-panel-sm" {...backdrop.panelProps}>
        <div className="modal-header">
          <h2 className="modal-title" id="vibenetwork-dialog-title-4">{editing ? t("vibeNetwork.editAlias") : t("vibeNetwork.addAlias")}</h2>
          <IconButton icon="x" size="sm" onClick={onClose} title={t("common.close")} />
        </div>
        <form className="server-form" onSubmit={handleSubmit}>
          <div className="modal-body">
            {error && <p className="form-note form-note-danger form-note-spaced">{error}</p>}
            {!editing && (
              <label className="form-field">
                <span className="form-label">{t("vibeNetwork.chooseApplication")}</span>
                <select className="form-input" value={applicationId} onChange={(e) => setApplicationId(e.target.value)}>
                  {applications.map((a) => (
                    <option key={a.id} value={a.id}>
                      {a.name}
                    </option>
                  ))}
                </select>
              </label>
            )}
            <label className="form-field">
              <span className="form-label">{t("vibeNetwork.hostname")}</span>
              <div className="vibe-network-hostname-row">
                <input className="form-input" value={hostname} onChange={(e) => setHostname(e.target.value)} placeholder="db01" />
                <span className="form-note">.vibe</span>
              </div>
            </label>
            <div className="form-actions">
              <Button type="button" variant="secondary" onClick={onClose} disabled={busy}>
                {t("common.cancel")}
              </Button>
              <Button type="submit" disabled={busy}>
                {busy ? t("common.saving") : t("common.save")}
              </Button>
            </div>
          </div>
        </form>
      </div>
    </div>
  );
}
