import { useTranslation } from "react-i18next";
import { Card } from "@/components/ui/Card";
import { Sparkline } from "@/components/ui/Sparkline";
import type { McSample, McStatus } from "@/hooks/useMinecraftStatus";
import "./MinecraftStatusCard.css";

/** Older than this and the snapshot is shown as stale rather than as the live truth. */
const STALE_AFTER_MS = 30_000;

/**
 * The Minecraft tab's body: a live view of a server running the ServerPulse plugin.
 *
 * Presentation only - {@link import("@/hooks/useMinecraftStatus").useMinecraftStatus} does the
 * reading and decides whether the tab is shown at all, so this is only ever handed a status
 * that exists. Each metric carries its own session sparkline, so the tab reads as a monitor
 * rather than a single frozen number.
 */
export function MinecraftStatusCard({ status, history }: { status: McStatus; history: McSample[] }) {
  const { t } = useTranslation();

  const stale = Date.now() - Date.parse(status.updatedAt) > STALE_AFTER_MS;
  const tps = status.tps.m1;
  const tpsTone = tps >= 19 ? "ok" : tps >= 15 ? "warn" : "bad";
  const ramPercent = status.memory.maxMb > 0 ? (status.memory.usedMb / status.memory.maxMb) * 100 : 0;

  return (
    <Card title={t("minecraft.title")} subtitle={`${status.server.software} ${status.server.version}`}>
      {stale && <p className="mc-stale">{t("minecraft.stale")}</p>}

      <div className="mc-grid">
        <div className={`mc-metric mc-tps-${tpsTone}`}>
          <span className="mc-metric-label">{t("minecraft.tps")}</span>
          <span className="mc-metric-value">{tps.toFixed(2)}</span>
          <span className="mc-metric-sub">
            5m {status.tps.m5.toFixed(1)} · 15m {status.tps.m15.toFixed(1)}
          </span>
          <div className="mc-metric-spark">
            <Sparkline values={history.map((sample) => sample.tps)} max={20} label={t("minecraft.tpsHistory")} />
          </div>
        </div>

        <div className="mc-metric">
          <span className="mc-metric-label">{t("minecraft.mspt")}</span>
          <span className="mc-metric-value">
            {status.msptAvg.toFixed(1)}
            <span className="mc-unit"> ms</span>
          </span>
          <span className="mc-metric-sub">&nbsp;</span>
          <div className="mc-metric-spark">
            <Sparkline values={history.map((sample) => sample.mspt)} label={t("minecraft.msptHistory")} />
          </div>
        </div>

        <div className="mc-metric">
          <span className="mc-metric-label">{t("minecraft.players")}</span>
          <span className="mc-metric-value">
            {status.players.online}
            <span className="mc-unit">/{status.players.max}</span>
          </span>
          <span className="mc-metric-sub">&nbsp;</span>
          <div className="mc-metric-spark">
            <Sparkline
              values={history.map((sample) => sample.players)}
              max={Math.max(1, status.players.max)}
              label={t("minecraft.playersHistory")}
            />
          </div>
        </div>

        <div className="mc-metric">
          <span className="mc-metric-label">{t("minecraft.ram")}</span>
          <span className="mc-metric-value">
            {Math.round(ramPercent)}
            <span className="mc-unit">%</span>
          </span>
          <span className="mc-metric-sub">
            {status.memory.usedMb} / {status.memory.maxMb} MB
          </span>
          <div className="mc-metric-spark">
            <Sparkline values={history.map((sample) => sample.ramPercent)} max={100} label={t("minecraft.ramHistory")} />
          </div>
        </div>
      </div>

      {status.worlds.length > 0 && (
        <table className="mc-worlds">
          <thead>
            <tr>
              <th>{t("minecraft.world")}</th>
              <th>{t("minecraft.players")}</th>
              <th>{t("minecraft.entities")}</th>
              <th>{t("minecraft.chunks")}</th>
            </tr>
          </thead>
          <tbody>
            {status.worlds.map((world) => (
              <tr key={world.name}>
                <td className="mc-world-name">{world.name}</td>
                <td>{world.players}</td>
                <td>{world.entities}</td>
                <td>{world.chunks}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

      {status.players.names && status.players.names.length > 0 && (
        <div className="mc-players-list">
          {status.players.names.map((name) => (
            <span key={name} className="mc-player">
              {name}
            </span>
          ))}
        </div>
      )}
    </Card>
  );
}
