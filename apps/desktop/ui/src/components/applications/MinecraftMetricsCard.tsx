import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Badge } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { Icon } from "@/components/ui/Icon";
import { getMinecraftMetrics, setMinecraftRconPassword } from "@/services/monitorService";
import type { MinecraftMetrics } from "@/types/serverEvent";
import { errorMessage } from "@/services/tauri";
import "@/components/servers/forms.css";
import "./MinecraftMetricsCard.css";

const DEFAULT_RCON_PORT = 25575;

/** Per-application, per-viewer convenience only - the port is not a secret,
 * and the password never touches this store (it lives in the OS keyring, set
 * through `setMinecraftRconPassword`). Wrapped because storage throws in a
 * private window. */
function storedPort(applicationId: string): number {
  try {
    const raw = localStorage.getItem(`vibessh-rcon-port:${applicationId}`);
    const parsed = raw ? Number.parseInt(raw, 10) : NaN;
    return Number.isInteger(parsed) && parsed > 0 && parsed <= 65535 ? parsed : DEFAULT_RCON_PORT;
  } catch {
    return DEFAULT_RCON_PORT;
  }
}

function rememberPort(applicationId: string, port: number): void {
  try {
    localStorage.setItem(`vibessh-rcon-port:${applicationId}`, String(port));
  } catch {
    // A viewer with storage blocked just retypes the port next time.
  }
}

/** The tone a TPS figure earns: 20 is healthy, below ~18 is where players
 * feel it. Teal-as-signal only for the good case, per the app's restraint. */
function tpsTone(tps: number | null): "success" | "neutral" | "danger" {
  if (tps === null) return "neutral";
  if (tps >= 19.5) return "success";
  if (tps >= 18) return "neutral";
  return "danger";
}

function formatTps(tps: number | null): string {
  return tps === null ? "—" : tps.toFixed(1);
}

interface MinecraftMetricsCardProps {
  applicationId: string;
  /** The server the application runs on - the RCON channel is opened over
   * that server's SSH session. */
  serverId: string;
}

/**
 * A Paper or Purpur server's own health - TPS, tick time and who is online -
 * read over RCON through the SSH tunnel, the way spark's panel shows it but
 * without spark's public link.
 *
 * Not on `ApplicationDetail`'s 5s status poll: an RCON round trip is a real
 * exchange over the tunnel, so it runs when the card mounts and when the user
 * asks again, like the health check beside it.
 */
export function MinecraftMetricsCard({ applicationId, serverId }: MinecraftMetricsCardProps) {
  const { t } = useTranslation();
  const [metrics, setMetrics] = useState<MinecraftMetrics | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [needsPassword, setNeedsPassword] = useState(false);
  const [editing, setEditing] = useState(false);
  const [port, setPort] = useState(() => storedPort(applicationId));
  const [passwordDraft, setPasswordDraft] = useState("");
  const [saving, setSaving] = useState(false);

  const load = useCallback(() => {
    setLoading(true);
    setError(null);
    getMinecraftMetrics(serverId, storedPort(applicationId))
      .then((result) => {
        setMetrics(result);
        setNeedsPassword(false);
      })
      .catch((err) => {
        const message = errorMessage(err, t);
        // A missing keyring entry is the first-run state, not a failure to
        // shout about - it opens the setup form instead of a red error.
        if (/rcon password/i.test(message)) {
          setNeedsPassword(true);
        } else {
          setError(message);
        }
      })
      .finally(() => setLoading(false));
  }, [applicationId, serverId, t]);

  useEffect(load, [load]);

  async function savePassword() {
    setSaving(true);
    setError(null);
    try {
      rememberPort(applicationId, port);
      await setMinecraftRconPassword(serverId, passwordDraft);
      setPasswordDraft("");
      setEditing(false);
      load();
    } catch (err) {
      setError(errorMessage(err, t));
    } finally {
      setSaving(false);
    }
  }

  const showSetup = needsPassword || editing;

  return (
    <Card title={t("minecraftMetrics.title")}>
      {showSetup ? (
        <div className="form-fields">
          <p className="form-hint">{t("minecraftMetrics.setupHint")}</p>
          <label className="form-field">
            <span className="form-label">{t("minecraftMetrics.portLabel")}</span>
            <input
              className="form-input"
              type="number"
              min={1}
              max={65535}
              value={port}
              onChange={(e) => setPort(Number.parseInt(e.target.value, 10) || DEFAULT_RCON_PORT)}
            />
          </label>
          <label className="form-field">
            <span className="form-label">{t("minecraftMetrics.passwordLabel")}</span>
            <input
              className="form-input"
              type="password"
              autoComplete="off"
              value={passwordDraft}
              onChange={(e) => setPasswordDraft(e.target.value)}
              placeholder={t("minecraftMetrics.passwordPlaceholder")}
            />
          </label>
          <div className="application-detail-actions">
            <Button variant="primary" size="sm" onClick={savePassword} disabled={saving || passwordDraft.length === 0}>
              {t("minecraftMetrics.save")}
            </Button>
            {editing && (
              <Button variant="secondary" size="sm" onClick={() => setEditing(false)} disabled={saving}>
                {t("minecraftMetrics.cancel")}
              </Button>
            )}
          </div>
          {error && <p className="form-error">{error}</p>}
        </div>
      ) : (
        <>
          <div className="application-detail-header-row">
            <Badge tone={metrics ? tpsTone(metrics.tps1m) : "neutral"}>
              {metrics && metrics.tps1m !== null
                ? t("minecraftMetrics.tpsNow", { tps: formatTps(metrics.tps1m) })
                : t("minecraftMetrics.tpsUnknown")}
            </Badge>
            <div className="application-detail-actions">
              <Button variant="secondary" size="sm" onClick={load} disabled={loading}>
                <Icon name="refresh-cw" size={14} />
                {t("minecraftMetrics.refresh")}
              </Button>
              <Button variant="secondary" size="sm" onClick={() => setEditing(true)} disabled={loading}>
                {t("minecraftMetrics.settings")}
              </Button>
            </div>
          </div>

          {error && <p className="form-error">{error}</p>}

          {metrics && (
            <dl className="minecraft-metrics-grid">
              <div>
                <dt>{t("minecraftMetrics.tps")}</dt>
                <dd>
                  {formatTps(metrics.tps1m)} / {formatTps(metrics.tps5m)} / {formatTps(metrics.tps15m)}
                  <span className="minecraft-metrics-unit">{t("minecraftMetrics.tpsWindows")}</span>
                </dd>
              </div>
              <div>
                <dt>{t("minecraftMetrics.mspt")}</dt>
                <dd>
                  {metrics.msptAvg === null
                    ? "—"
                    : t("minecraftMetrics.msptValue", {
                        avg: metrics.msptAvg.toFixed(1),
                        max: (metrics.msptMax ?? 0).toFixed(1),
                      })}
                </dd>
              </div>
              <div>
                <dt>{t("minecraftMetrics.players")}</dt>
                <dd>
                  {metrics.playersOnline} / {metrics.playersMax}
                  {metrics.playerNames.length > 0 && (
                    <span className="minecraft-metrics-names">{metrics.playerNames.join(", ")}</span>
                  )}
                </dd>
              </div>
            </dl>
          )}
        </>
      )}
    </Card>
  );
}
