import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { Icon } from "@/components/ui/Icon";
import { useServersStore } from "@/stores/serversStore";
import {
  cancelAgentPairing,
  generatePairingCode,
  getPairingCodeTtlSeconds,
  onAgentPairingEvent,
  onAgentPairingState,
  startAgentPairing,
} from "@/services/pairingService";
import type { AgentConnectionState } from "@/types/pairing";
import type { ServerMetrics } from "@/types/serverEvent";
import { CapabilityBadges } from "./CapabilityBadges";
import { MetricsPreview } from "./MetricsPreview";
import "./forms.css";

const INSTALL_URL = "https://raw.githubusercontent.com/VibeSSH/vibessh/main/agent-install/install.sh";

interface AgentPairingFlowProps {
  onPaired: () => void;
}

/**
 * The WebSocket connection this opens lives only as long as this component
 * is mounted - unmounting calls cancelAgentPairing(), including when the
 * user clicks "Done" and the parent modal closes. The live MetricsPreview
 * below is real, pushed data (Etap J), but it's a preview of the pipeline
 * working, not a persistent per-server session - there's no Dashboard/
 * session-manager to hand this connection off to yet (that's downstream of
 * server storage, Etap 2). Reopening "Add Server" reconnects from scratch.
 */

export function AgentPairingFlow({ onPaired }: AgentPairingFlowProps) {
  const { t } = useTranslation();
  const [host, setHost] = useState("");
  const [port, setPort] = useState("7420");
  const [code, setCode] = useState<string | null>(null);
  const [remainingSeconds, setRemainingSeconds] = useState<number | null>(null);
  const [connectionState, setConnectionState] = useState<AgentConnectionState | null>(null);
  const [busy, setBusy] = useState(false);
  const [backendError, setBackendError] = useState<string | null>(null);
  const [latestMetrics, setLatestMetrics] = useState<ServerMetrics | null>(null);
  const ttlRef = useRef<number>(300);
  const upsertServer = useServersStore((s) => s.upsertServer);

  useEffect(() => {
    getPairingCodeTtlSeconds()
      .then((ttl) => {
        ttlRef.current = ttl;
      })
      .catch(() => {});

    const unlistenPromise = onAgentPairingState((state) => {
      setConnectionState(state);
      if (state.status === "connected") {
        upsertServer({
          id: state.agentId,
          name: host || state.agentId.slice(0, 8),
          host,
          connectionMode: "agent",
          status: "online",
          agentId: state.agentId,
          agentVersion: state.agentVersion,
          capabilities: state.capabilities,
        });
      }
    });

    const unlistenEventsPromise = onAgentPairingEvent((event) => {
      if (event.type === "metrics.update") {
        setLatestMetrics(event.metrics);
      }
    });

    return () => {
      unlistenPromise.then((unlisten) => unlisten());
      unlistenEventsPromise.then((unlisten) => unlisten());
      cancelAgentPairing().catch(() => {});
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (remainingSeconds === null || connectionState?.status === "connected") return;
    if (remainingSeconds <= 0) return;
    const id = window.setInterval(() => {
      setRemainingSeconds((s) => (s === null ? null : Math.max(0, s - 1)));
    }, 1000);
    return () => window.clearInterval(id);
  }, [remainingSeconds, connectionState]);

  async function handleGenerate() {
    setBusy(true);
    setBackendError(null);
    try {
      const newCode = await generatePairingCode();
      setCode(newCode);
      setRemainingSeconds(ttlRef.current);
      setConnectionState(null);
      setLatestMetrics(null);
      await startAgentPairing(host, Number(port), newCode);
    } catch (err) {
      setBackendError(err instanceof Error ? err.message : t("agentPairing.errorBackend"));
    } finally {
      setBusy(false);
    }
  }

  async function handleCopy(text: string) {
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      // clipboard access denied - nothing useful to do about it here
    }
  }

  const expired = remainingSeconds === 0 && connectionState?.status !== "connected";
  const minutes = remainingSeconds !== null ? Math.floor(remainingSeconds / 60) : 0;
  const seconds = remainingSeconds !== null ? remainingSeconds % 60 : 0;

  return (
    <div className="server-form">
      <div className="form-row">
        <label className="form-field form-field-grow">
          <span className="form-label">{t("agentPairing.host")}</span>
          <input
            className="form-input"
            placeholder="203.0.113.10"
            value={host}
            onChange={(e) => setHost(e.target.value)}
            disabled={connectionState?.status === "connected"}
          />
        </label>
        <label className="form-field form-field-narrow">
          <span className="form-label">{t("agentPairing.port")}</span>
          <input
            className="form-input"
            value={port}
            onChange={(e) => setPort(e.target.value)}
            disabled={connectionState?.status === "connected"}
          />
        </label>
      </div>

      <div className="form-field">
        <span className="form-label">{t("agentPairing.step1Title")}</span>
        <div className="code-block">
          <span className="code-block-text">curl -fsSL {INSTALL_URL} | sudo sh</span>
          <button
            type="button"
            className="code-block-copy"
            onClick={() => handleCopy(`curl -fsSL ${INSTALL_URL} | sudo sh`)}
            aria-label={t("agentPairing.copyCommandAria")}
          >
            <Icon name="copy" size={14} />
          </button>
        </div>
        <p className="form-note">{t("agentPairing.noteInstall")}</p>
      </div>

      <div className="form-field">
        <span className="form-label">{t("agentPairing.step2Title")}</span>
        {!code ? (
          <>
            <Button onClick={handleGenerate} disabled={busy || !host}>
              <Icon name="key" size={16} />
              {t("agentPairing.generateCode")}
            </Button>
            {backendError && (
              <p className="form-note" style={{ color: "var(--danger)" }}>
                {backendError}
              </p>
            )}
          </>
        ) : (
          <>
            <div className="pairing-code-display">
              <span className="pairing-code-value">{code}</span>
              {connectionState?.status !== "connected" && remainingSeconds !== null && (
                <span className={`pairing-code-timer ${remainingSeconds < 60 ? "pairing-code-timer-low" : ""}`}>
                  {expired ? t("agentPairing.expired") : `${minutes}:${seconds.toString().padStart(2, "0")}`}
                </span>
              )}
            </div>

            <div
              className={`pairing-status ${
                connectionState?.status === "connected"
                  ? "pairing-status-connected"
                  : connectionState?.status === "disconnected" && expired
                  ? "pairing-status-error"
                  : ""
              }`}
            >
              {connectionState?.status === "connected" ? (
                <>
                  <Icon name="check" size={16} />
                  {t("agentPairing.connected", { version: connectionState.agentVersion })}
                </>
              ) : connectionState?.status === "disconnected" ? (
                <>
                  <Icon name="x" size={16} />
                  {connectionState.reason}
                </>
              ) : expired ? (
                <>
                  <Icon name="x" size={16} />
                  {t("agentPairing.codeExpired")}
                </>
              ) : (
                <>
                  <span className="pairing-spinner" />
                  {t("agentPairing.waiting")}
                </>
              )}
            </div>

            {connectionState?.status === "connected" && (
              <>
                <CapabilityBadges capabilities={connectionState.capabilities} />
                {latestMetrics ? <MetricsPreview metrics={latestMetrics} /> : <p className="form-note">{t("agentPairing.waitingMetrics")}</p>}
              </>
            )}

            {connectionState?.status !== "connected" && (
              <Button variant="secondary" onClick={handleGenerate} disabled={busy || !host}>
                {t("agentPairing.generateNewCode")}
              </Button>
            )}
          </>
        )}
      </div>

      {connectionState?.status === "connected" && (
        <div className="form-actions">
          <Button onClick={onPaired}>{t("agentPairing.done")}</Button>
        </div>
      )}
    </div>
  );
}
