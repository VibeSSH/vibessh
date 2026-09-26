import { useCallback, useEffect, useMemo, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { Badge } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { Dialog } from "@/components/ui/Dialog";
import { EmptyState } from "@/components/ui/EmptyState";
import { Icon } from "@/components/ui/Icon";
import { IconButton } from "@/components/ui/IconButton";
import { Select } from "@/components/ui/Select";
import { SkeletonRows } from "@/components/ui/SkeletonRows";
import { Switch } from "@/components/ui/Switch";
import {
  createApplicationSchedule,
  deleteApplicationSchedule,
  installCron,
  listApplicationSchedules,
  runApplicationScheduleNow,
  updateApplicationSchedule,
  type ApplicationSchedule,
  type ApplicationSchedules,
  type ScheduleAction,
  type ScheduleInput,
} from "@/services/scheduleService";
import { CommandError, errorMessage } from "@/services/tauri";
import { toastError, toastSuccess } from "@/stores/toastStore";
import { buildCron, isValidCron, nextRun, presetOf, twoDigits, type CronPreset } from "@/utils/cron";
import "./SchedulesTab.css";

const ACTIONS: ScheduleAction[] = ["restart", "stop", "start"];
/** Monday first, the way a Polish calendar reads; cron's Sunday is 0. */
const WEEK = [1, 2, 3, 4, 5, 6, 0];

/** A weekday's short name in the viewer's language, from cron's 0-6. */
function weekdayName(day: number, language: string): string {
  // 2026-09-27 was a Sunday, so adding `day` lands on the right weekday.
  return new Intl.DateTimeFormat(language, { weekday: "short", timeZone: "UTC" }).format(new Date(Date.UTC(2026, 8, 27 + day)));
}

/** A time on the Node's clock, and the same moment on the viewer's, when they differ. */
function useDescribe(nodeOffset: number | null) {
  const { t, i18n } = useTranslation();
  return useCallback(
    (expression: string) => {
      const preset = presetOf(expression);
      const time = (hour: number, minute: number) => {
        const node = `${twoDigits(hour)}:${twoDigits(minute)}`;
        const localOffset = -new Date().getTimezoneOffset();
        if (nodeOffset === null || nodeOffset === localOffset) return node;
        const minutes = (((hour * 60 + minute - nodeOffset + localOffset) % 1440) + 1440) % 1440;
        return t("schedules.timeBoth", { node, local: `${twoDigits(Math.floor(minutes / 60))}:${twoDigits(minutes % 60)}` });
      };
      switch (preset.kind) {
        case "daily":
          return t("schedules.describeDaily", { time: time(preset.hour, preset.minute) });
        case "weekdays":
          return t("schedules.describeWeekdays", {
            days: WEEK.filter((day) => preset.days.includes(day)).map((day) => weekdayName(day, i18n.language)).join(", "),
            time: time(preset.hour, preset.minute),
          });
        case "hours":
          return t("schedules.describeHours", { count: preset.every });
        case "custom":
          return t("schedules.describeCustom", { expression: preset.expression });
      }
    },
    [nodeOffset, t, i18n.language],
  );
}

/**
 * Scheduled power actions for one Application - see the backend's
 * `services::schedule_service`.
 *
 * They run on the Node from its own cron, so this tab is where they are set
 * and checked on, not what runs them: closing VibeSSH changes nothing. That
 * is said on the tab itself, because the other scheduled thing in the app -
 * backups - works the opposite way.
 */
export function SchedulesTab({ applicationId, canManage }: { applicationId: string; canManage: boolean }) {
  const { t, i18n } = useTranslation();
  const [data, setData] = useState<ApplicationSchedules | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [editing, setEditing] = useState<ApplicationSchedule | "new" | null>(null);
  const [deleting, setDeleting] = useState<ApplicationSchedule | null>(null);
  /** The schedule with a request in flight, so only its row shows busy. */
  const [busy, setBusy] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      setData(await listApplicationSchedules(applicationId));
      setLoadError(null);
    } catch (err) {
      setLoadError(errorMessage(err, t));
    }
  }, [applicationId, t]);

  useEffect(() => {
    void load();
  }, [load]);

  const nodeOffset = data?.timeZone?.offsetMinutes ?? null;
  const describe = useDescribe(nodeOffset);
  const lastRunOf = useMemo(() => new Map((data?.lastRuns ?? []).map((run) => [run.scheduleId, run])), [data]);
  const formatWhen = (date: Date) => date.toLocaleString(i18n.language, { weekday: "short", day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" });

  async function toggle(schedule: ApplicationSchedule, enabled: boolean) {
    setBusy(schedule.id);
    try {
      await updateApplicationSchedule(schedule.id, { name: schedule.name, cron: schedule.cron, action: schedule.action, enabled });
      await load();
    } catch (err) {
      toastError(errorMessage(err, t));
    } finally {
      setBusy(null);
    }
  }

  async function runNow(schedule: ApplicationSchedule) {
    setBusy(schedule.id);
    try {
      await runApplicationScheduleNow(schedule.id);
      toastSuccess(t("schedules.ranNowToast", { name: schedule.name }));
    } catch (err) {
      toastError(errorMessage(err, t));
    } finally {
      setBusy(null);
      await load();
    }
  }

  const timeZoneNote = data?.timeZone
    ? t("schedules.timeZoneNote", {
        zone: data.timeZone.name ?? "",
        offset: `UTC${data.timeZone.offsetMinutes >= 0 ? "+" : "-"}${twoDigits(Math.floor(Math.abs(data.timeZone.offsetMinutes) / 60))}:${twoDigits(Math.abs(data.timeZone.offsetMinutes) % 60)}`,
      })
    : null;

  return (
    <>
      <Card
        title={t("schedules.title")}
        subtitle={t("schedules.subtitle")}
        actions={
          canManage && (
            <Button size="sm" onClick={() => setEditing("new")} disabled={!data}>
              <Icon name="plus" size={14} />
              {t("schedules.add")}
            </Button>
          )
        }
      >
        {loadError && <p className="form-note form-note-danger form-note-spaced">{loadError}</p>}
        {data?.nodeError && <p className="form-note form-note-danger form-note-spaced">{t("schedules.nodeError", { reason: data.nodeError })}</p>}
        {!data && !loadError ? (
          <SkeletonRows />
        ) : data && data.schedules.length === 0 ? (
          <EmptyState icon="history" title={t("schedules.emptyTitle")} description={t("schedules.emptyDescription")} />
        ) : (
          data && (
            <ul className="schedule-rows">
              {data.schedules.map((schedule) => {
                const next = schedule.enabled && nodeOffset !== null ? nextRun(schedule.cron, new Date(), nodeOffset) : null;
                const last = lastRunOf.get(schedule.id);
                return (
                  <li key={schedule.id} className={`schedule-row${schedule.enabled ? "" : " schedule-row-off"}`}>
                    <div className="schedule-main">
                      <span className="schedule-name">
                        {schedule.name}
                        <Badge tone="neutral">{t(`schedules.action.${schedule.action}`)}</Badge>
                      </span>
                      <span className="schedule-when">{describe(schedule.cron)}</span>
                      <span className="schedule-meta">
                        {schedule.enabled
                          ? next
                            ? t("schedules.next", { when: formatWhen(next) })
                            : nodeOffset === null
                              ? null
                              : t("schedules.never")
                          : t("schedules.paused")}
                        {last && (
                          <span className={last.exitCode === 0 ? "schedule-last" : "schedule-last schedule-last-failed"} title={last.message || undefined}>
                            <Icon name={last.exitCode === 0 ? "check" : "alert-triangle"} size={12} />
                            {last.exitCode === 0
                              ? t("schedules.lastOk", { when: formatWhen(new Date(last.ranAt)) })
                              : t("schedules.lastFailed", { when: formatWhen(new Date(last.ranAt)), reason: last.message })}
                          </span>
                        )}
                      </span>
                    </div>
                    <div className="schedule-actions">
                      {canManage && (
                        <>
                          <IconButton
                            icon="play"
                            size="sm"
                            title={t("schedules.runNow", { action: t(`schedules.action.${schedule.action}`) })}
                            disabled={busy === schedule.id}
                            onClick={() => void runNow(schedule)}
                          />
                          <IconButton icon="edit" size="sm" title={t("schedules.edit")} disabled={busy === schedule.id} onClick={() => setEditing(schedule)} />
                          <IconButton icon="trash" size="sm" danger title={t("schedules.delete")} disabled={busy === schedule.id} onClick={() => setDeleting(schedule)} />
                        </>
                      )}
                      <Switch
                        checked={schedule.enabled}
                        disabled={!canManage || busy === schedule.id}
                        ariaLabel={t(schedule.enabled ? "schedules.pauseAria" : "schedules.resumeAria", { name: schedule.name })}
                        onChange={(enabled) => void toggle(schedule, enabled)}
                      />
                    </div>
                  </li>
                );
              })}
            </ul>
          )
        )}
        {timeZoneNote && <p className="schedule-note">{timeZoneNote}</p>}
        <p className="schedule-note">
          <Icon name="server" size={13} />
          {t("schedules.runsOnNode")}
        </p>
      </Card>

      {editing && (
        <ScheduleEditor
          applicationId={applicationId}
          schedule={editing === "new" ? null : editing}
          nodeOffset={nodeOffset}
          onClose={() => setEditing(null)}
          onSaved={async () => {
            setEditing(null);
            await load();
          }}
        />
      )}

      {deleting && (
        <DeleteScheduleDialog
          schedule={deleting}
          onClose={() => setDeleting(null)}
          onDeleted={async () => {
            setDeleting(null);
            await load();
          }}
        />
      )}
    </>
  );
}

function ScheduleEditor({
  applicationId,
  schedule,
  nodeOffset,
  onClose,
  onSaved,
}: {
  applicationId: string;
  schedule: ApplicationSchedule | null;
  nodeOffset: number | null;
  onClose: () => void;
  onSaved: () => Promise<void>;
}) {
  const { t, i18n } = useTranslation();
  const initial = schedule ? presetOf(schedule.cron) : ({ kind: "daily", hour: 4, minute: 0 } as CronPreset);
  const [name, setName] = useState(schedule?.name ?? t("schedules.defaultName"));
  const [action, setAction] = useState<ScheduleAction>(schedule?.action ?? "restart");
  const [kind, setKind] = useState<CronPreset["kind"]>(initial.kind);
  const [time, setTime] = useState(
    initial.kind === "daily" || initial.kind === "weekdays" ? `${twoDigits(initial.hour)}:${twoDigits(initial.minute)}` : "04:00",
  );
  const [days, setDays] = useState<number[]>(initial.kind === "weekdays" ? initial.days : [1, 2, 3, 4, 5]);
  const [every, setEvery] = useState(initial.kind === "hours" ? String(initial.every) : "6");
  const [custom, setCustom] = useState(schedule?.cron ?? "0 4 * * *");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  /** The Node to install cron on, when saving found it has none. */
  const [cronMissingOn, setCronMissingOn] = useState<string | null>(null);
  const [installingCron, setInstallingCron] = useState(false);

  const [hour, minute] = time.split(":").map(Number);
  const expression =
    kind === "daily"
      ? buildCron({ kind, hour, minute })
      : kind === "weekdays"
        ? buildCron({ kind, hour, minute, days })
        : kind === "hours"
          ? buildCron({ kind, every: Number(every) })
          : buildCron({ kind, expression: custom });
  const valid = isValidCron(expression) && !(kind === "weekdays" && days.length === 0);
  const preview = valid && nodeOffset !== null ? nextRun(expression, new Date(), nodeOffset) : null;

  async function submit(event?: FormEvent) {
    event?.preventDefault();
    if (!valid) return;
    setSaving(true);
    setError(null);
    setCronMissingOn(null);
    const input: ScheduleInput = { name, cron: expression, action, enabled: schedule?.enabled ?? true };
    try {
      if (schedule) await updateApplicationSchedule(schedule.id, input);
      else await createApplicationSchedule(applicationId, input);
      toastSuccess(t("schedules.savedToast", { name }));
      await onSaved();
    } catch (err) {
      setError(errorMessage(err, t));
      if (err instanceof CommandError && err.code === "cron_missing" && typeof err.params.serverId === "string") {
        setCronMissingOn(err.params.serverId);
      }
    } finally {
      setSaving(false);
    }
  }

  /** Installs cron, then saves again - the save is what the person asked for. */
  async function installAndRetry() {
    if (!cronMissingOn) return;
    setInstallingCron(true);
    try {
      await installCron(cronMissingOn);
      toastSuccess(t("schedules.cronInstalledToast"));
      await submit();
    } catch (err) {
      setError(errorMessage(err, t));
    } finally {
      setInstallingCron(false);
    }
  }

  return (
    <Dialog open onClose={onClose} size="sm" dismissable={!saving && !installingCron} title={schedule ? t("schedules.editTitle") : t("schedules.addTitle")}>
      <form className="modal-body server-form" onSubmit={submit}>
        <label className="form-field">
          <span className="form-label">{t("schedules.fieldName")}</span>
          <input className="form-input" value={name} maxLength={80} onChange={(event) => setName(event.target.value)} required />
        </label>

        <div className="form-field">
          <span className="form-label">{t("schedules.fieldAction")}</span>
          <Select
            value={action}
            onChange={(value) => setAction(value as ScheduleAction)}
            items={ACTIONS.map((value) => ({ value, label: t(`schedules.action.${value}`) }))}
            aria-label={t("schedules.fieldAction")}
          />
        </div>

        <div className="form-field">
          <span className="form-label">{t("schedules.fieldWhen")}</span>
          <div className="schedule-kinds" role="radiogroup" aria-label={t("schedules.fieldWhen")}>
            {(["daily", "weekdays", "hours", "custom"] as const).map((value) => (
              <button
                key={value}
                type="button"
                role="radio"
                aria-checked={kind === value}
                className={`schedule-kind${kind === value ? " schedule-kind-active" : ""}`}
                onClick={() => setKind(value)}
              >
                {t(`schedules.kind.${value}`)}
              </button>
            ))}
          </div>
        </div>

        {(kind === "daily" || kind === "weekdays") && (
          <label className="form-field">
            <span className="form-label">{t("schedules.fieldTime")}</span>
            <input className="form-input schedule-time" type="time" value={time} onChange={(event) => setTime(event.target.value || "00:00")} required />
          </label>
        )}

        {kind === "weekdays" && (
          <div className="schedule-days" role="group" aria-label={t("schedules.fieldDays")}>
            {WEEK.map((day) => (
              <button
                key={day}
                type="button"
                aria-pressed={days.includes(day)}
                className={`schedule-day${days.includes(day) ? " schedule-day-active" : ""}`}
                onClick={() => setDays((current) => (current.includes(day) ? current.filter((d) => d !== day) : [...current, day]))}
              >
                {weekdayName(day, i18n.language)}
              </button>
            ))}
          </div>
        )}

        {kind === "hours" && (
          <div className="form-field">
            <span className="form-label">{t("schedules.fieldEvery")}</span>
            <Select
              value={every}
              onChange={setEvery}
              items={["1", "2", "3", "4", "6", "8", "12"].map((value) => ({ value, label: t("schedules.everyHours", { count: Number(value) }) }))}
              aria-label={t("schedules.fieldEvery")}
            />
          </div>
        )}

        {kind === "custom" && (
          <label className="form-field">
            <span className="form-label">{t("schedules.fieldCron")}</span>
            <input className="form-input schedule-cron" value={custom} onChange={(event) => setCustom(event.target.value)} spellCheck={false} />
            <span className="form-note">{t("schedules.cronHelp")}</span>
          </label>
        )}

        <p className={`form-note${valid ? "" : " form-note-danger"}`}>
          {!valid
            ? kind === "weekdays" && days.length === 0
              ? t("schedules.pickADay")
              : t("schedules.invalidCron")
            : preview
              ? t("schedules.preview", {
                  when: preview.toLocaleString(i18n.language, { weekday: "long", day: "numeric", month: "long", hour: "2-digit", minute: "2-digit" }),
                })
              : null}
        </p>
        {action !== "start" && <p className="form-note">{t("schedules.graceNote")}</p>}
        {error && <p className="form-note form-note-danger">{error}</p>}
        {cronMissingOn && (
          <Button type="button" variant="secondary" size="sm" onClick={() => void installAndRetry()} disabled={installingCron || saving}>
            <Icon name="download" size={14} />
            {installingCron ? t("schedules.installingCron") : t("schedules.installCron")}
          </Button>
        )}

        <div className="form-actions">
          <Button type="button" variant="secondary" onClick={onClose} disabled={saving || installingCron}>
            {t("common.cancel")}
          </Button>
          <Button type="submit" disabled={saving || installingCron || !valid || !name.trim()}>
            {saving ? t("common.saving") : t("common.save")}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}

function DeleteScheduleDialog({ schedule, onClose, onDeleted }: { schedule: ApplicationSchedule; onClose: () => void; onDeleted: () => Promise<void> }) {
  const { t } = useTranslation();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function confirm() {
    setBusy(true);
    setError(null);
    try {
      await deleteApplicationSchedule(schedule.id);
      await onDeleted();
    } catch (err) {
      setError(errorMessage(err, t));
      setBusy(false);
    }
  }

  return (
    <Dialog open onClose={onClose} size="sm" dismissable={!busy} title={t("schedules.deleteTitle")}>
      <div className="modal-body">
        <p className="dialog-body-text">{t("schedules.deleteBody", { name: schedule.name })}</p>
        {error && <p className="form-note form-note-danger form-note-spaced">{error}</p>}
        <div className="form-actions">
          <Button variant="secondary" onClick={onClose} disabled={busy}>
            {t("common.cancel")}
          </Button>
          <Button variant="danger" onClick={() => void confirm()} disabled={busy}>
            {t("schedules.delete")}
          </Button>
        </div>
      </div>
    </Dialog>
  );
}
