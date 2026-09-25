import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Card } from "@/components/ui/Card";
import { Icon } from "@/components/ui/Icon";
import { LivePill } from "@/components/ui/LivePill";
import type { SchedulerStatus } from "@/hooks/useSchedulerStatus";
import "./SchedulerStatusCard.css";

/** Older than this and the schedule is shown as stale rather than as the live truth. */
const STALE_AFTER_MS = 30_000;

type Translate = (key: string, options?: Record<string, unknown>) => string;

/**
 * The Restarts tab's body: a live view of a server running the VibeSSH Scheduler plugin.
 *
 * Presentation only - {@link import("@/hooks/useSchedulerStatus").useSchedulerStatus} does the
 * reading and decides whether the tab is shown at all, so this is only ever handed a status that
 * exists. The countdown ticks down every second on its own, anchored on the moment the reading
 * arrived ({@link fetchedAt}) rather than on the two machines' clocks, so it stays smooth between
 * the page's polls without drifting.
 *
 * Laid out like the Minecraft tab: the countdown on the left, and beside it when that is in
 * words ("tomorrow, 06:00" over the full date) and how the restart is carried out. The method
 * used to be printed twice, as the card's subtitle and again as a fact, and the date came with
 * its seconds.
 */
export function SchedulerStatusCard({ status, fetchedAt }: { status: SchedulerStatus; fetchedAt: number }) {
  const { t, i18n } = useTranslation();
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
  const spigot = status.method === "SPIGOT_RESTART";
  const reason = describeReason(status.reason, t);

  return (
    <Card
      title={t("restarts.title")}
      subtitle={t("restarts.subtitle")}
      actions={<LivePill stale={stale} liveLabel={t("minecraft.live")} staleLabel={t("minecraft.staleShort")} />}
    >
      {stale && <p className="restart-stale">{t("restarts.stale")}</p>}

      {!scheduled ? (
        <p className="restart-empty">{t("restarts.none")}</p>
      ) : (
        <div className="restart-layout">
          <div className={`restart-hero restart-tone-${tone}`}>
            <span className="restart-hero-label">{t("restarts.nextIn")}</span>
            <span className="restart-hero-value">{formatDuration(remaining, t)}</span>
            <span className="restart-reason">
              <Icon name={reason.icon} size={13} />
              {reason.label}
            </span>
          </div>

          <div className="restart-facts">
            <div className="restart-fact">
              <span className="restart-fact-label">
                <Icon name="history" size={13} />
                {t("restarts.when")}
              </span>
              <span className="restart-fact-value">{describeWhen(status.nextRestart as string, i18n.language, t)}</span>
              <span className="restart-fact-sub">
                {new Date(status.nextRestart as string).toLocaleDateString(i18n.language, { day: "numeric", month: "long", year: "numeric" })}
              </span>
            </div>
            <div className="restart-fact">
              <span className="restart-fact-label">
                <Icon name="power" size={13} />
                {t("restarts.method")}
              </span>
              <span className="restart-fact-value">{spigot ? t("restarts.methodSpigot") : t("restarts.methodShutdown")}</span>
              <span className="restart-fact-sub">{spigot ? t("restarts.methodSpigotHelp") : t("restarts.methodShutdownHelp")}</span>
            </div>
          </div>
        </div>
      )}
    </Card>
  );
}

/** Renders whole seconds as {@code 1h 05m}, {@code 4m 30s}, {@code 12s} or the "now" label. */
function formatDuration(seconds: number, t: Translate): string {
  if (seconds <= 0) return t("restarts.now");
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const secs = seconds % 60;
  if (hours > 0) return `${hours}h ${String(minutes).padStart(2, "0")}m`;
  if (minutes > 0) return `${minutes}m ${String(secs).padStart(2, "0")}s`;
  return `${secs}s`;
}

/** "today, 06:00", "tomorrow, 06:00", or the weekday for anything further out. */
function describeWhen(iso: string, locale: string, t: Translate): string {
  const when = new Date(iso);
  const time = when.toLocaleTimeString(locale, { hour: "2-digit", minute: "2-digit" });
  const startOfDay = (date: Date) => new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime();
  const days = Math.round((startOfDay(when) - startOfDay(new Date())) / 86_400_000);
  if (days === 0) return t("restarts.today", { time });
  if (days === 1) return t("restarts.tomorrow", { time });
  return `${when.toLocaleDateString(locale, { weekday: "long" })}, ${time}`;
}

/**
 * Turns the plugin's English reason into the viewer's language, with an icon for its kind.
 *
 * The plugin writes a small set of reasons ("scheduled restart", "manual restart", "low TPS x.x")
 * because plugins are English-only; the panel is bilingual, so they are mapped back here. Anything
 * unrecognised falls back to the scheduled wording rather than showing raw English.
 */
function describeReason(reason: string | null, t: Translate): { icon: string; label: string } {
  if (reason) {
    const lowTps = reason.match(/low tps\s*([\d.]+)/i);
    if (lowTps) return { icon: "alert-triangle", label: t("restarts.reasonLowTps", { tps: lowTps[1] }) };
    if (/manual/i.test(reason)) return { icon: "user", label: t("restarts.reasonManual") };
  }
  return { icon: "history", label: t("restarts.reasonScheduled") };
}
