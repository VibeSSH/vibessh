import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { listApplicationSchedules, type ApplicationSchedules, type ScheduleAction } from "@/services/scheduleService";
import { nextRun } from "@/utils/cron";
import "./UpcomingActionsCard.css";

/** How often the schedules are read again while the Overview is open. They
 *  change only when somebody edits them, and each read is a few SSH calls. */
const RELOAD_MS = 5 * 60_000;
const SHOWN = 3;

interface Upcoming {
  key: string;
  label: string;
  action: ScheduleAction;
  at: Date;
  /** From the VibeSSH Scheduler plugin rather than a schedule of ours. */
  plugin: boolean;
}

/**
 * What is going to happen to this application next - the Overview's answer to
 * "when does it restart?" without opening another tab.
 *
 * Gathers both kinds of timed action the app knows about: its own schedules,
 * which the Node's cron runs, and the next restart the VibeSSH Scheduler
 * plugin has announced. Read, not managed, here: the button goes to the tab
 * that manages them.
 */
export function UpcomingActionsCard({
  applicationId,
  schedulesAvailable,
  pluginNextRestart,
  onManage,
}: {
  applicationId: string;
  schedulesAvailable: boolean;
  /** ISO instant from the plugin's status file, when the plugin is there. */
  pluginNextRestart: string | null;
  onManage: () => void;
}) {
  const { t, i18n } = useTranslation();
  const [data, setData] = useState<ApplicationSchedules | null>(null);
  // Re-rendered each minute so "next" moves on once a run has passed.
  const [now, setNow] = useState(() => new Date());

  useEffect(() => {
    if (!schedulesAvailable) return;
    let cancelled = false;
    const load = () =>
      listApplicationSchedules(applicationId)
        .then((result) => {
          if (!cancelled) setData(result);
        })
        .catch((err) => console.warn("couldn't read the schedules for the overview", err));
    void load();
    const reload = window.setInterval(() => void load(), RELOAD_MS);
    const tick = window.setInterval(() => setNow(new Date()), 60_000);
    return () => {
      cancelled = true;
      window.clearInterval(reload);
      window.clearInterval(tick);
    };
  }, [applicationId, schedulesAvailable]);

  const offset = data?.timeZone?.offsetMinutes ?? null;
  const upcoming: Upcoming[] = [];
  if (data && offset !== null) {
    for (const schedule of data.schedules) {
      if (!schedule.enabled) continue;
      const at = nextRun(schedule.cron, now, offset);
      if (at) upcoming.push({ key: schedule.id, label: schedule.name, action: schedule.action, at, plugin: false });
    }
  }
  if (pluginNextRestart) {
    const at = new Date(pluginNextRestart);
    if (!Number.isNaN(at.getTime()) && at > now) {
      upcoming.push({ key: "plugin", label: t("upcoming.pluginLabel"), action: "restart", at, plugin: true });
    }
  }
  upcoming.sort((a, b) => a.at.getTime() - b.at.getTime());

  if (!schedulesAvailable && upcoming.length === 0) return null;

  const when = (at: Date) => {
    const time = at.toLocaleTimeString(i18n.language, { hour: "2-digit", minute: "2-digit" });
    const day = (offsetDays: number) => {
      const date = new Date(now);
      date.setDate(date.getDate() + offsetDays);
      return date.toDateString();
    };
    if (at.toDateString() === day(0)) return t("upcoming.today", { time });
    if (at.toDateString() === day(1)) return t("upcoming.tomorrow", { time });
    return at.toLocaleString(i18n.language, { weekday: "short", day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" });
  };

  return (
    <Card
      title={t("upcoming.title")}
      actions={
        schedulesAvailable && (
          <Button variant="secondary" size="sm" onClick={onManage}>
            {t("applicationDetail.managePorts")}
          </Button>
        )
      }
    >
      {upcoming.length === 0 ? (
        <p className="form-note">{schedulesAvailable && !data ? t("upcoming.loading") : t("upcoming.none")}</p>
      ) : (
        <ul className="upcoming-list">
          {upcoming.slice(0, SHOWN).map((item) => (
            <li key={item.key} className="upcoming-item">
              <span className="upcoming-main">
                <span className="upcoming-action">{t(`schedules.action.${item.action}`)}</span>
                <span className="upcoming-label">{item.label}</span>
              </span>
              <span className="upcoming-when">{when(item.at)}</span>
            </li>
          ))}
        </ul>
      )}
    </Card>
  );
}
