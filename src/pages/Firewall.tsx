import { useCallback, useEffect, useState } from "react";
import { Navigate, useNavigate, useParams } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { Badge } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { Checkbox } from "@/components/ui/Checkbox";
import { EmptyState } from "@/components/ui/EmptyState";
import { HostAddress } from "@/components/ui/HostAddress";
import { Icon } from "@/components/ui/Icon";
import { IconButton } from "@/components/ui/IconButton";
import { SkeletonRows } from "@/components/ui/SkeletonRows";
import { useModalDialog } from "@/hooks/useModalDialog";
import {
  addFirewallCustomRule,
  enableServerFirewall,
  getNodeFirewallOverview,
  removeFirewallCustomRule,
  syncNodeFirewall,
  type FirewallCustomRuleInput,
  type FirewallRuleOrigin,
  type NodeFirewallOverview,
} from "@/services/serverService";
import { useServersStore } from "@/stores/serversStore";
import { toastSuccess } from "@/stores/toastStore";
import "./pages.css";
import "@/components/servers/AddServerModal.css";
import "@/components/servers/forms.css";
import { errorMessage } from "@/services/tauri";

function originLabel(t: (key: string, opts?: Record<string, unknown>) => string, origin: FirewallRuleOrigin): string {
  switch (origin.kind) {
    case "ssh":
      return t("firewallPage.originSsh");
    case "wireGuard":
      return t("firewallPage.originWireGuard");
    case "application":
      return t("firewallPage.originApplication", { name: origin.applicationName, port: origin.portName });
    case "custom":
      return origin.label || t("firewallPage.originCustom");
  }
}

export function FirewallPage() {
  const { t } = useTranslation();
  const { serverId } = useParams<{ serverId: string }>();
  const navigate = useNavigate();
  const server = useServersStore((s) => s.servers.find((srv) => srv.id === serverId));

  const [overview, setOverview] = useState<NodeFirewallOverview | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [syncing, setSyncing] = useState(false);
  const [enabling, setEnabling] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const [addOpen, setAddOpen] = useState(false);
  const [deletingRule, setDeletingRule] = useState<{ id: string; label: string } | null>(null);
  const [deleting, setDeleting] = useState(false);

  const load = useCallback(() => {
    if (!serverId) return;
    setLoading(true);
    setLoadError(null);
    getNodeFirewallOverview(serverId)
      .then(setOverview)
      .catch((err) => setLoadError(errorMessage(err, t)))
      .finally(() => setLoading(false));
  }, [serverId, t]);

  useEffect(load, [load]);

  if (!serverId) {
    return <Navigate to="/servers" replace />;
  }

  async function handleSync() {
    setSyncing(true);
    setActionError(null);
    try {
      await syncNodeFirewall(serverId!);
      load();
    } catch (err) {
      setActionError(errorMessage(err, t));
    } finally {
      setSyncing(false);
    }
  }

  async function handleEnable() {
    setEnabling(true);
    setActionError(null);
    try {
      const result = await enableServerFirewall(serverId!);
      if (!result.backend) {
        setActionError(t("firewallPage.noBackend"));
        return;
      }
      toastSuccess(t("firewallPage.securedToast"));
      load();
    } catch (err) {
      setActionError(errorMessage(err, t));
    } finally {
      setEnabling(false);
    }
  }

  async function handleConfirmDelete() {
    if (!deletingRule) return;
    setDeleting(true);
    try {
      await removeFirewallCustomRule(serverId!, deletingRule.id);
      setDeletingRule(null);
      load();
    } catch (err) {
      setActionError(errorMessage(err, t));
    } finally {
      setDeleting(false);
    }
  }

  return (
    <div className="page">
      <div className="page-header page-header-row">
        <div>
          <h1 className="page-title">{server ? server.name : t("nav.firewall")}</h1>
          <p className="page-subtitle">{server ? <HostAddress value={server.host} /> : serverId}</p>
        </div>
        <Button variant="secondary" onClick={() => navigate("/servers")}>
          <Icon name="chevron-left" size={16} />
          {t("common.backToServers")}
        </Button>
      </div>

      {loadError && <p className="page-error-note">{loadError}</p>}
      {actionError && <p className="page-error-note">{actionError}</p>}

      <Card title={t("firewallPage.statusTitle")}>
        {loading ? (
          <SkeletonRows />
        ) : (
          overview && (
            <>
              <div className="wizard-review-grid">
                <span className="wizard-review-label">{t("firewallPage.backend")}</span>
                <span className="wizard-review-value">{overview.backend ?? t("firewallPage.noBackendShort")}</span>
                <span className="wizard-review-label">{t("firewallPage.enforcement")}</span>
                <span className="wizard-review-value">
                  <Badge tone={overview.active ? "success" : "neutral"}>{overview.active ? t("firewallPage.active") : t("firewallPage.inactive")}</Badge>
                </span>
              </div>
              <div className="form-actions">
                <Button variant="secondary" size="sm" onClick={handleSync} disabled={syncing}>
                  <Icon name="refresh-cw" size={14} />
                  {syncing ? t("common.loading") : t("firewallPage.syncNow")}
                </Button>
                {!overview.active && (
                  <Button size="sm" onClick={handleEnable} disabled={enabling}>
                    <Icon name="lock" size={14} />
                    {enabling ? t("common.loading") : t("firewallPage.secure")}
                  </Button>
                )}
              </div>
            </>
          )
        )}
      </Card>

      <Card title={t("firewallPage.rulesTitle")} subtitle={overview ? t("firewallPage.rulesCount", { count: overview.rules.length }) : undefined}>
        <div className="application-detail-header-row">
          <p className="form-note">{t("firewallPage.rulesNote")}</p>
          <Button size="sm" onClick={() => setAddOpen(true)}>
            <Icon name="plus" size={14} />
            {t("firewallPage.addCustomRule")}
          </Button>
        </div>

        {loading ? (
          <SkeletonRows />
        ) : !overview || overview.rules.length === 0 ? (
          <EmptyState icon="lock" title={t("firewallPage.emptyTitle")} description={t("firewallPage.emptyDescription")} />
        ) : (
          <ul className="server-list">
            {overview.rules.map((view) => (
              <li key={`${view.port}-${view.protocol}-${view.sourceCidr ?? ""}`} className="server-list-item">
                <div className="server-list-main">
                  <span className="server-list-name">
                    {view.port}/{view.protocol.toUpperCase()}
                    {view.sourceCidr ? ` · ${view.sourceCidr}` : ""}
                  </span>
                  <span className="server-list-host">{originLabel(t, view.origin)}</span>
                </div>
                {view.origin.kind === "custom" && (
                  <IconButton
                    icon="trash"
                    size="sm"
                    danger
                    title={t("firewallPage.deleteRuleAria")}
                    onClick={() => setDeletingRule({ id: view.origin.kind === "custom" ? view.origin.ruleId : "", label: originLabel(t, view.origin) })}
                  />
                )}
              </li>
            ))}
          </ul>
        )}
      </Card>

      {addOpen && (
        <AddCustomRuleModal
          serverId={serverId}
          onClose={() => setAddOpen(false)}
          onAdded={() => {
            setAddOpen(false);
            load();
          }}
        />
      )}

      {deletingRule && (
        <DeleteFirewallRuleDialog
          label={deletingRule.label}
          busy={deleting}
          onCancel={() => !deleting && setDeletingRule(null)}
          onConfirm={handleConfirmDelete}
        />
      )}
    </div>
  );
}

interface AddCustomRuleModalProps {
  serverId: string;
  onClose: () => void;
  onAdded: () => void;
}

function AddCustomRuleModal({ serverId, onClose, onAdded }: AddCustomRuleModalProps) {
  const { t } = useTranslation();
  const backdrop = useModalDialog(onClose, { labelledBy: "firewall-dialog-title-1" });
  const [label, setLabel] = useState("");
  const [port, setPort] = useState("");
  const [protocol, setProtocol] = useState<"tcp" | "udp">("tcp");
  const [restrictSource, setRestrictSource] = useState(false);
  const [sourceCidr, setSourceCidr] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    const portNumber = Number(port);
    if (!Number.isInteger(portNumber) || portNumber < 1 || portNumber > 65535) {
      setError(t("firewallPage.invalidPort"));
      return;
    }
    if (restrictSource && !sourceCidr.trim()) {
      setError(t("firewallPage.invalidCidr"));
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const input: FirewallCustomRuleInput = {
        label: label.trim() || undefined,
        protocol,
        port: portNumber,
        sourceCidr: restrictSource ? sourceCidr.trim() : undefined,
      };
      await addFirewallCustomRule(serverId, input);
      onAdded();
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
          <h2 className="modal-title" id="firewall-dialog-title-1">{t("firewallPage.addCustomRuleTitle")}</h2>
          <IconButton icon="x" size="sm" onClick={onClose} title={t("common.close")} />
        </div>
        <form className="server-form" onSubmit={handleSubmit}>
          <div className="modal-body">
            {error && <p className="form-note form-note-danger form-note-spaced">{error}</p>}
            <label className="form-field">
              <span className="form-label">{t("firewallPage.ruleLabel")}</span>
              <input className="form-input" value={label} onChange={(e) => setLabel(e.target.value)} placeholder={t("firewallPage.ruleLabelPlaceholder")} autoFocus />
            </label>
            <div className="form-row">
              <label className="form-field">
                <span className="form-label">{t("firewallPage.rulePort")}</span>
                <input className="form-input" type="number" min={1} max={65535} value={port} onChange={(e) => setPort(e.target.value)} placeholder="8443" />
              </label>
              <label className="form-field">
                <span className="form-label">{t("firewallPage.ruleProtocol")}</span>
                <div className="form-segmented">
                  <button type="button" className={`form-segment ${protocol === "tcp" ? "form-segment-active" : ""}`} onClick={() => setProtocol("tcp")}>
                    TCP
                  </button>
                  <button type="button" className={`form-segment ${protocol === "udp" ? "form-segment-active" : ""}`} onClick={() => setProtocol("udp")}>
                    UDP
                  </button>
                </div>
              </label>
            </div>
            <Checkbox checked={restrictSource} onChange={setRestrictSource} label={t("firewallPage.restrictSource")} />
            {restrictSource && (
              <label className="form-field">
                <span className="form-label">{t("firewallPage.ruleSourceCidr")}</span>
                <input className="form-input" value={sourceCidr} onChange={(e) => setSourceCidr(e.target.value)} placeholder="10.77.0.0/16" />
              </label>
            )}
            <p className="form-note">{t("firewallPage.addRuleNote")}</p>
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

/**
 * Its own component rather than JSX inside a conditional, because
 * `useModalDialog` has an effect that must run on the dialog's own mount -
 * see `EnvironmentTab`'s copy of this note.
 */
function DeleteFirewallRuleDialog({
  label,
  busy,
  onCancel,
  onConfirm,
}: {
  label: string | null;
  busy: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const { t } = useTranslation();
  const dialog = useModalDialog(onCancel, { labelledBy: "delete-firewall-rule-title" });
  return (
    <div className="modal-backdrop" {...dialog.backdropProps}>
      <div className="modal-panel modal-panel-sm" {...dialog.panelProps}>
        <div className="modal-header">
          <h2 className="modal-title" id="delete-firewall-rule-title">
            {t("firewallPage.deleteRuleTitle")}
          </h2>
          <IconButton icon="x" size="sm" onClick={onCancel} title={t("common.close")} disabled={busy} />
        </div>
        <div className="modal-body">
          <p className="dialog-body-text">{t("firewallPage.deleteRuleBody", { label })}</p>
          <div className="form-actions">
            <Button variant="secondary" onClick={onCancel} disabled={busy}>
              {t("common.cancel")}
            </Button>
            <Button variant="danger" onClick={onConfirm} disabled={busy}>
              {busy ? t("common.loading") : t("common.remove")}
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}
