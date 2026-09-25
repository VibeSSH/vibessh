import { useTranslation } from "react-i18next";
import { Card } from "@/components/ui/Card";
import { Icon } from "@/components/ui/Icon";
import { LivePill } from "@/components/ui/LivePill";
import { MetricTile, MetricTileGrid, type MetricTone } from "@/components/ui/MetricTile";
import type { McSample, McStatus } from "@/hooks/useMinecraftStatus";
import "./MinecraftStatusCard.css";

/** Older than this and the snapshot is shown as stale rather than as the live truth. */
const STALE_AFTER_MS = 30_000;

/** A server tick's time budget: 20 ticks a second leaves 50 ms for each. */
const TICK_BUDGET_MS = 50;

type Tone = MetricTone;

const tpsTone = (tps: number): Tone => (tps >= 19 ? "ok" : tps >= 15 ? "warn" : "bad");
const msptTone = (mspt: number): Tone => (mspt < 30 ? "ok" : mspt < 45 ? "warn" : "bad");

function formatUptime(totalSeconds: number): string {
  const days = Math.floor(totalSeconds / 86_400);
  const hours = Math.floor((totalSeconds % 86_400) / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  if (days > 0) return `${days}d ${hours}h`;
  if (hours > 0) return `${hours}h ${minutes}m`;
  return `${minutes}m`;
}

/**
 * The Minecraft tab's body: a live view of a server running the VibeSSH Metrics plugin.
 *
 * Presentation only - {@link import("@/hooks/useMinecraftStatus").useMinecraftStatus} does the
 * reading and decides whether the tab is shown at all, so this is only ever handed a status
 * that exists.
 *
 * Every tile has the same shape - label, value, one line of context, a chart on a fixed
 * scale - so the four charts line up and each one's height means something: TPS out of 20,
 * MSPT out of the 50 ms a tick has, players out of the slots, memory out of 100%. MSPT used
 * to scale to its own peak, which drew 0.5-0.7 ms of noise as if the server were struggling.
 */
export function MinecraftStatusCard({ status, history }: { status: McStatus; history: McSample[] }) {
  const { t } = useTranslation();

  const stale = Date.now() - Date.parse(status.updatedAt) > STALE_AFTER_MS;
  const tps = status.tps.m1;
  const mspt = status.msptAvg;
  const ramPercent = status.memory.maxMb > 0 ? (status.memory.usedMb / status.memory.maxMb) * 100 : 0;
  const names = status.players.names;

  return (
    <Card
      title={t("minecraft.title")}
      subtitle={`${status.server.software} ${status.server.version} · ${t("minecraft.uptime", { time: formatUptime(status.server.uptimeSeconds) })}`}
      actions={<LivePill stale={stale} liveLabel={t("minecraft.live")} staleLabel={t("minecraft.staleShort")} />}
    >
      {stale && <p className="mc-stale">{t("minecraft.stale")}</p>}

      <MetricTileGrid>
        <MetricTile
          label={t("minecraft.tps")}
          value={tps.toFixed(2)}
          tone={tpsTone(tps)}
          sub={`5m ${status.tps.m5.toFixed(1)} · 15m ${status.tps.m15.toFixed(1)}`}
          spark={{ values: history.map((sample) => sample.tps), max: 20, label: t("minecraft.tpsHistory") }}
        />
        <MetricTile
          label={t("minecraft.mspt")}
          value={mspt.toFixed(1)}
          unit="ms"
          tone={msptTone(mspt) === "ok" ? undefined : msptTone(mspt)}
          sub={t("minecraft.msptBudget", { budget: TICK_BUDGET_MS })}
          spark={{ values: history.map((sample) => sample.mspt), max: TICK_BUDGET_MS, label: t("minecraft.msptHistory") }}
        />
        <MetricTile
          label={t("minecraft.players")}
          value={status.players.online}
          unit={`/${status.players.max}`}
          sub={t("minecraft.playersSub", { free: Math.max(0, status.players.max - status.players.online) })}
          spark={{ values: history.map((sample) => sample.players), max: Math.max(1, status.players.max), label: t("minecraft.playersHistory") }}
        />
        <MetricTile
          label={t("minecraft.ram")}
          value={Math.round(ramPercent)}
          unit="%"
          sub={`${status.memory.usedMb} / ${status.memory.maxMb} MB`}
          spark={{ values: history.map((sample) => sample.ramPercent), max: 100, label: t("minecraft.ramHistory") }}
        />
      </MetricTileGrid>

      <div className="mc-panels">
        <section className="mc-panel">
          <h4 className="mc-panel-title">
            <Icon name="layout-grid" size={14} />
            {t("minecraft.worlds")}
            <span className="mc-panel-count">{status.worlds.length}</span>
          </h4>
          {status.worlds.length === 0 ? (
            <p className="mc-panel-empty">{t("minecraft.noWorlds")}</p>
          ) : (
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
                    <td className={world.players > 0 ? "mc-cell-active" : undefined}>{world.players}</td>
                    <td>{world.entities}</td>
                    <td>{world.chunks}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </section>

        <section className="mc-panel">
          <h4 className="mc-panel-title">
            <Icon name="users" size={14} />
            {t("minecraft.onlinePlayers")}
            <span className="mc-panel-count">{status.players.online}</span>
          </h4>
          {names == null ? (
            <p className="mc-panel-empty">{t("minecraft.namesHidden")}</p>
          ) : names.length === 0 ? (
            <p className="mc-panel-empty">{t("minecraft.nobodyOnline")}</p>
          ) : (
            <div className="mc-players-list">
              {names.map((name) => (
                <span key={name} className="mc-player">
                  {name}
                </span>
              ))}
            </div>
          )}
        </section>
      </div>
    </Card>
  );
}
