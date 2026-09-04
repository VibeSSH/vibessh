import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { copyToClipboard } from "@/utils/copyToClipboard";
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
import { startAgentSession, upgradeServerToAgent, upsertAgentServer } from "@/services/serverService";
import type { AgentConnectionState } from "@/types/pairing";
import type { ServerMetrics } from "@/types/serverEvent";
import { CapabilityBadges } from "./CapabilityBadges";
import { MetricsPreview } from "./MetricsPreview";
import "./forms.css";
import { errorMessage } from "@/services/tauri";

const INSTALL_URL = "https://raw.githubusercontent.com/VibeSSH/vibessh/main/agent-install/install.sh";

interface AgentPairingFlowProps {
  onPaired: () => void;
  /** When set, a successful pairing upgrades this already-known SSH-mode server in place (same id, `host` pre-filled and locked - we already know exactly which machine this is) instead of creating a brand new row - the Setup Page's own "also install the Vibe Agent" step. */
  upgradeExistingServer?: { id: string; host: string };
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

export function AgentPairingFlow({ onPaired, upgradeExistingServer }: AgentPairingFlowProps) {
  const { t } = useTranslation();
  const [host, setHost] = useState(upgradeExistingServer?.host ?? "");
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

    const unlistenPromise = onAgentPairingState(async (state) => {
      setConnectionState(state);
      if (state.status !== "connected") return;

      const name = host || state.agentId.slice(0, 8);
      try {
        // Persists a real row so this server survives past this session -
        // previously agent-paired servers only ever lived in this
        // component's own upsertServer call below, gone the moment the app
        // closed (see README's own "still only show for the current
        // session" note on Etap H). `upgradeExistingServer` set means this
        // pairing is for a Node the user already added over SSH (the Setup
        // Page's own "also install the Vibe Agent" step) - upgrade that
        // same row in place instead of creating a confusing second entry
        // for the same physical machine.
        const persisted = upgradeExistingServer
          ? await upgradeServerToAgent(upgradeExistingServer.id, state.agentId, state.capabilities.docker)
          : await upsertAgentServer(name, host, state.agentId, state.capabilities.docker);
        // Hands the connection off to a persistent, app-session-long one
        // (Etap M3) - this is the one moment host/port/a fresh credential
        // are all in hand at once, see AgentSessionManager's own doc
        // comment. Best-effort: a failure here just means this Node stays
        // reachable only through this modal's own connection, exactly like
        // before Etap M3 existed - not worth surfacing as a pairing error.
        if (state.issuedCredential) {
          startAgentSession(persisted.id, host, Number(port), state.issuedCredential).catch(() => {});
        }
        upsertServer({
          id: persisted.id,
          name: persisted.name,
          host: persisted.host,
          connectionMode: "agent",
          status: "online",
          agentId: state.agentId,
          agentVersion: state.agentVersion,
          capabilities: state.capabilities,
          createdAt: persisted.createdAt,
        });
      } catch {
        // Persistence failed (e.g. this is running outside a real Tauri
        // webview, as it does for this app's own browser-preview UI checks)
        // - the live connection still works, just without surviving a
        // restart, same as before this was ever persisted at all.
        upsertServer({
          id: state.agentId,
          name,
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
      setBackendError(errorMessage(err, t));
    } finally {
      setBusy(false);
    }
  }

  async function handleCopy(text: string) {
    await copyToClipboard(text, { copied: t("common.copied"), failed: t("common.copyFailed") });
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
            disabled={Boolean(upgradeExistingServer) || connectionState?.status === "connected"}
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
