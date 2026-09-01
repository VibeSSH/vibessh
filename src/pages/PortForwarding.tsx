import { useCallback, useEffect, useState, type FormEvent } from "react";
import { Navigate, useNavigate, useParams } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { Badge } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { EmptyState } from "@/components/ui/EmptyState";
import { HostAddress } from "@/components/ui/HostAddress";
import { Icon } from "@/components/ui/Icon";
import { IconButton } from "@/components/ui/IconButton";
import { SkeletonRows } from "@/components/ui/SkeletonRows";
import { useBackdropClose } from "@/hooks/useBackdropClose";
import { listPortForwards, startPortForward, stopPortForward } from "@/services/portForwardService";
import { useServersStore } from "@/stores/serversStore";
import type { PortForwardKind, PortForwardStatus, StartPortForwardInput } from "@/types/portForward";
import "./pages.css";
import "@/components/servers/AddServerModal.css";
import "@/components/servers/forms.css";

function kindLabel(t: (key: string) => string, kind: PortForwardKind): string {
  switch (kind) {
    case "local":
      return t("portForwardingPage.kindLocal");
    case "remote":
      return t("portForwardingPage.kindRemote");
    case "dynamic":
      return t("portForwardingPage.kindDynamic");
  }
}

export function PortForwardingPage() {
  const { t } = useTranslation();
  const { serverId } = useParams<{ serverId: string }>();
  const navigate = useNavigate();
  const server = useServersStore((s) => s.servers.find((srv) => srv.id === serverId));

  const [forwards, setForwards] = useState<PortForwardStatus[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [addOpen, setAddOpen] = useState(false);
  const [stoppingId, setStoppingId] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);

  const load = useCallback(() => {
    setLoading(true);
    setLoadError(null);
    listPortForwards()
      .then(setForwards)
      .catch((err) => setLoadError(err instanceof Error ? err.message : t("portForwardingPage.loadError")))
      .finally(() => setLoading(false));
  }, [t]);

  useEffect(load, [load]);

  if (!serverId) {
    return <Navigate to="/servers" replace />;
  }

  const serverForwards = forwards.filter((forward) => forward.serverId === serverId);

  async function handleStop(id: string) {
    setStoppingId(id);
    setActionError(null);
    try {
      await stopPortForward(id);
      load();
    } catch (err) {
      setActionError(err instanceof Error ? err.message : t("portForwardingPage.stopError"));
    } finally {
      setStoppingId(null);
    }
  }

  return (
    <div className="page">
      <div className="page-header page-header-row">
        <div>
          <h1 className="page-title">{server ? server.name : t("nav.portForwarding")}</h1>
          <p className="page-subtitle">{server ? <HostAddress value={server.host} /> : serverId}</p>
        </div>
        <Button variant="secondary" onClick={() => navigate("/servers")}>
          <Icon name="chevron-left" size={16} />
          {t("common.backToServers")}
        </Button>
      </div>

      {loadError && <p className="page-error-note">{loadError}</p>}
      {actionError && <p className="page-error-note">{actionError}</p>}

      <Card title={t("portForwardingPage.title")} subtitle={t("portForwardingPage.subtitle")}>
        <div className="application-detail-header-row">
          <p className="form-note">{t("portForwardingPage.note")}</p>
          <Button size="sm" onClick={() => setAddOpen(true)}>
            <Icon name="plus" size={14} />
            {t("portForwardingPage.addForward")}
          </Button>
        </div>

        {loading ? (
          <SkeletonRows />
        ) : serverForwards.length === 0 ? (
          <EmptyState icon="arrow-left-right" title={t("portForwardingPage.emptyTitle")} description={t("portForwardingPage.emptyDescription")} />
        ) : (
          <ul className="server-list">
            {serverForwards.map((forward) => (
              <li key={forward.id} className="server-list-item">
                <div className="server-list-main">
                  <span className="server-list-name">
                    <Badge tone="neutral">{kindLabel(t, forward.kind)}</Badge> {forward.bindAddress}:{forward.bindPort}
                    {forward.kind !== "dynamic" && forward.targetHost ? ` → ${forward.targetHost}:${forward.targetPort}` : ""}
                  </span>
                  <span className="server-list-host">
                    {forward.kind === "dynamic"
                      ? t("portForwardingPage.socksNote")
                      : forward.kind === "remote"
                        ? t("portForwardingPage.remoteNote")
                        : t("portForwardingPage.localNote")}
                  </span>
                </div>
                <IconButton
                  icon="x"
                  size="sm"
                  danger
                  title={t("portForwardingPage.stopAria")}
                  onClick={() => handleStop(forward.id)}
                  disabled={stoppingId === forward.id}
                />
              </li>
            ))}
          </ul>
        )}
      </Card>

      {addOpen && (
        <AddForwardModal
          serverId={serverId}
          onClose={() => setAddOpen(false)}
          onAdded={() => {
            setAddOpen(false);
            load();
          }}
        />
      )}
    </div>
  );
}

interface AddForwardModalProps {
  serverId: string;
  onClose: () => void;
  onAdded: () => void;
}

function AddForwardModal({ serverId, onClose, onAdded }: AddForwardModalProps) {
  const { t } = useTranslation();
  const backdrop = useBackdropClose(onClose);
  const [kind, setKind] = useState<PortForwardKind>("local");
  const [bindAddress, setBindAddress] = useState("127.0.0.1");
  const [bindPort, setBindPort] = useState("");
  const [targetHost, setTargetHost] = useState("");
  const [targetPort, setTargetPort] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    const bindPortNumber = bindPort.trim() === "" ? 0 : Number(bindPort);
    if (!Number.isInteger(bindPortNumber) || bindPortNumber < 0 || bindPortNumber > 65535) {
      setError(t("portForwardingPage.invalidBindPort"));
      return;
    }
    let targetPortNumber: number | undefined;
    if (kind !== "dynamic") {
      targetPortNumber = Number(targetPort);
      if (!targetHost.trim() || !Number.isInteger(targetPortNumber) || targetPortNumber < 1 || targetPortNumber > 65535) {
        setError(t("portForwardingPage.invalidTarget"));
        return;
      }
    }
    setBusy(true);
    setError(null);
    try {
      const input: StartPortForwardInput = {
        serverId,
        kind,
        bindAddress: bindAddress.trim() || "127.0.0.1",
        bindPort: bindPortNumber,
        targetHost: kind !== "dynamic" ? targetHost.trim() : undefined,
        targetPort: kind !== "dynamic" ? targetPortNumber : undefined,
      };
      await startPortForward(input);
      onAdded();
    } catch (err) {
      setError(err instanceof Error ? err.message : t("portForwardingPage.startError"));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="modal-backdrop" {...backdrop}>
      <div className="modal-panel modal-panel-sm" onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <h2 className="modal-title">{t("portForwardingPage.addForwardTitle")}</h2>
          <IconButton icon="x" size="sm" onClick={onClose} title={t("common.close")} />
        </div>
        <form className="server-form" onSubmit={handleSubmit}>
          <div className="modal-body">
            {error && <p className="form-note form-note-danger form-note-spaced">{error}</p>}

            <label className="form-field">
              <span className="form-label">{t("portForwardingPage.kindLabel")}</span>
              <div className="form-segmented">
                <button type="button" className={`form-segment ${kind === "local" ? "form-segment-active" : ""}`} onClick={() => setKind("local")}>
                  {t("portForwardingPage.kindLocal")}
                </button>
                <button type="button" className={`form-segment ${kind === "remote" ? "form-segment-active" : ""}`} onClick={() => setKind("remote")}>
                  {t("portForwardingPage.kindRemote")}
                </button>
                <button type="button" className={`form-segment ${kind === "dynamic" ? "form-segment-active" : ""}`} onClick={() => setKind("dynamic")}>
                  {t("portForwardingPage.kindDynamic")}
                </button>
              </div>
            </label>
            <p className="form-note">
              {kind === "local" ? t("portForwardingPage.localHelp") : kind === "remote" ? t("portForwardingPage.remoteHelp") : t("portForwardingPage.dynamicHelp")}
            </p>

            <div className="form-row">
              <label className="form-field">
                <span className="form-label">{kind === "remote" ? t("portForwardingPage.bindAddressRemote") : t("portForwardingPage.bindAddressLocal")}</span>
                <input className="form-input" value={bindAddress} onChange={(e) => setBindAddress(e.target.value)} placeholder="127.0.0.1" />
              </label>
              <label className="form-field">
                <span className="form-label">{t("portForwardingPage.bindPort")}</span>
                <input
                  className="form-input"
                  type="number"
                  min={0}
                  max={65535}
                  value={bindPort}
                  onChange={(e) => setBindPort(e.target.value)}
                  placeholder={t("portForwardingPage.bindPortPlaceholder")}
                />
              </label>
            </div>

            {kind !== "dynamic" && (
              <div className="form-row">
                <label className="form-field">
                  <span className="form-label">{t("portForwardingPage.targetHost")}</span>
                  <input
                    className="form-input"
                    value={targetHost}
                    onChange={(e) => setTargetHost(e.target.value)}
                    placeholder={kind === "local" ? "db01.vibe" : "127.0.0.1"}
                  />
                </label>
                <label className="form-field">
                  <span className="form-label">{t("portForwardingPage.targetPort")}</span>
                  <input className="form-input" type="number" min={1} max={65535} value={targetPort} onChange={(e) => setTargetPort(e.target.value)} placeholder="3306" />
                </label>
              </div>
            )}

            <div className="form-actions">
              <Button type="button" variant="secondary" onClick={onClose} disabled={busy}>
                {t("common.cancel")}
              </Button>
              <Button type="submit" disabled={busy}>
                {busy ? t("common.loading") : t("portForwardingPage.start")}
              </Button>
            </div>
          </div>
        </form>
      </div>
    </div>
  );
}
