import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { MigrationPhase, MigrationProgress } from "@/services/applicationService";
import { formatBytes } from "@/utils/formatBytes";
import "./MigrationProgressView.css";

const PHASES: MigrationPhase[] = ["stopping", "preparing", "scanning", "copying", "starting", "finishing"];

/** How much of the recent past the transfer rate is averaged over. */
const RATE_WINDOW_MS = 5000;

/**
 * What a running migration is doing, while it does it.
 *
 * The copy is the long part and gets the bar, by bytes; the steps around it
 * are quick and are shown as a step count with a moving stripe, because
 * there is nothing in them to measure.
 */
export function MigrationProgressView({ progress }: { progress: MigrationProgress | null }) {
  const { t } = useTranslation();
  const rate = useTransferRate(progress);

  const phase = progress?.phase ?? "preparing";
  const step = PHASES.indexOf(phase) + 1;
  const copying = progress?.phase === "copying" && progress.bytesTotal > 0;
  const percent = copying ? Math.min(100, Math.floor((progress.bytesDone / progress.bytesTotal) * 100)) : null;
  const remainingSeconds =
    copying && rate && rate > 0 ? Math.max(0, Math.round((progress.bytesTotal - progress.bytesDone) / rate)) : null;

  return (
    <div className="migration-progress" role="status" aria-live="polite">
      <div className="migration-progress-head">
        <span className="migration-progress-phase">{t(`applicationDetail.migratePhase.${phase}`)}</span>
        <span className="migration-progress-step">
          {percent !== null ? `${percent}%` : t("applicationDetail.migrateStep", { step, total: PHASES.length })}
        </span>
      </div>

      <div
        className={`migration-progress-track${percent === null ? " migration-progress-track-indeterminate" : ""}`}
        role="progressbar"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={percent ?? undefined}
      >
        <div className="migration-progress-fill" style={percent !== null ? { width: `${percent}%` } : undefined} />
      </div>

      {copying && progress && (
        <>
          <div className="migration-progress-stats">
            <span>{t("applicationDetail.migrateBytes", { done: formatBytes(progress.bytesDone), total: formatBytes(progress.bytesTotal) })}</span>
            <span>{t("applicationDetail.migrateFiles", { done: progress.filesDone, total: progress.filesTotal })}</span>
            {rate !== null && <span>{t("applicationDetail.migrateRate", { rate: formatBytes(rate) })}</span>}
            {remainingSeconds !== null && <span>{t("applicationDetail.migrateRemaining", { time: formatDuration(remainingSeconds) })}</span>}
          </div>
          {progress.current && (
            <p className="migration-progress-current" title={progress.current}>
              {progress.current}
            </p>
          )}
        </>
      )}
      <p className="migration-progress-note">{t("applicationDetail.migrateKeepOpen")}</p>
    </div>
  );
}

/** Bytes per second over the last few seconds of the copy, or null before there is enough to tell. */
function useTransferRate(progress: MigrationProgress | null): number | null {
  const samples = useRef<Array<{ at: number; bytes: number }>>([]);
  const [rate, setRate] = useState<number | null>(null);

  useEffect(() => {
    if (!progress || progress.phase !== "copying") {
      samples.current = [];
      setRate(null);
      return;
    }
    const now = Date.now();
    const list = samples.current;
    list.push({ at: now, bytes: progress.bytesDone });
    while (list.length > 2 && now - list[0].at > RATE_WINDOW_MS) list.shift();
    const first = list[0];
    const elapsed = (now - first.at) / 1000;
    setRate(elapsed >= 1 ? (progress.bytesDone - first.bytes) / elapsed : null);
  }, [progress]);

  return rate;
}

function formatDuration(seconds: number): string {
  if (seconds < 60) return `${seconds} s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes} min ${seconds % 60} s`;
  return `${Math.floor(minutes / 60)} h ${minutes % 60} min`;
}
