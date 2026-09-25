import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Card } from "@/components/ui/Card";
import type { SchedulerStatus } from "@/hooks/useSchedulerStatus";
import "./SchedulerStatusCard.css";

/** Older than this and the schedule is shown as stale rather than as the live truth. */
const STALE_AFTER_MS = 30_000;

/**
 * The Restarts tab's body: a live view of a server running the VibeSSH Scheduler plugin.
 *
 * Presentation only - {@link import("@/hooks/useSchedulerStatus").useSchedulerStatus} does the
 * reading and decides whether the tab is shown at all, so this is only ever handed a status that
 * exists. The countdown ticks down every second on its own, anchored on the moment the reading
 * arrived ({@link fetchedAt}) rather than on the two machines' clocks, so it stays smooth between
 * the page's polls without drifting.
 */
export function SchedulerStatusCard({ status, fetchedAt }: { status: SchedulerStatus; fetchedAt: number }) {
  const { t } = useTranslation();
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, []);

  const stale = now - Date.parse(status.updatedAt) > STALE_AFTER_MS;
  const scheduled = status.nextRestart !== null && status.secondsUntil >= 0;
  const elapsed = Math.max(0, (now - fetchedAt) / 1000);
  const remaining = Math.max(0, Math.round(status.secondsUntil - elapsed));
  const tone = remaining <= 10 ? "bad" : remaining <= 60 ? "warn" : "ok";
  const methodLabel =
    status.method === "SPIGOT_RESTART" ? t("restarts.methodSpigot") : t("restarts.methodShutdown");

  return (
    <Card title={t("restarts.title")} subtitle={methodLabel}>
      {stale && <p className="restart-stale">{t("restarts.stale")}</p>}

      {!scheduled ? (
        <p className="restart-empty">{t("restarts.none")}</p>
      ) : (
        <>
          <div className={`restart-hero restart-tone-${tone}`}>
            <span className="restart-hero-label">{t("restarts.nextIn")}</span>
            <span className="restart-hero-value">{formatDuration(remaining, t)}</span>
            <span className="restart-hero-reason">{translateReason(status.reason, t)}</span>
          </div>

          <div className="restart-facts">
            <div className="restart-fact">
              <span className="restart-fact-label">{t("restarts.when")}</span>
              <span className="restart-fact-value">{new Date(status.nextRestart as string).toLocaleString()}</span>
            </div>
            <div className="restart-fact">
              <span className="restart-fact-label">{t("restarts.method")}</span>
              <span className="restart-fact-value">{methodLabel}</span>
            </div>
          </div>
        </>
      )}
    </Card>
  );
}

/** Renders whole seconds as {@code 1h 05m}, {@code 4m 30s}, {@code 12s} or the "now" label. */
function formatDuration(seconds: number, t: (key: string) => string): string {
  if (seconds <= 0) return t("restarts.now");
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const secs = seconds % 60;
  if (hours > 0) return `${hours}h ${String(minutes).padStart(2, "0")}m`;
  if (minutes > 0) return `${minutes}m ${String(secs).padStart(2, "0")}s`;
  return `${secs}s`;
}

/**
 * Turns the plugin's English reason into the viewer's language.
 *
 * The plugin writes a small set of reasons ("scheduled restart", "manual restart", "low TPS x.x")
 * because plugins are English-only; the panel is bilingual, so they are mapped back here. Anything
 * unrecognised falls back to the scheduled wording rather than showing raw English.
 */
function translateReason(reason: string | null, t: (key: string, opts?: Record<string, unknown>) => string): string {
  if (reason) {
    const lowTps = reason.match(/low tps\s*([\d.]+)/i);
    if (lowTps) return t("restarts.reasonLowTps", { tps: lowTps[1] });
    if (/manual/i.test(reason)) return t("restarts.reasonManual");
  }
  return t("restarts.reasonScheduled");
}
