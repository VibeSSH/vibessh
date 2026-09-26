import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Icon } from "@/components/ui/Icon";
import { getApplicationDiskUsage, type DiskUsage } from "@/services/scheduleService";
import { formatBytes } from "@/utils/formatBytes";
import "./DiskUsageBar.css";

/** The Node checks every five minutes; asking more often would only re-read the same record. */
const RELOAD_MS = 5 * 60_000;

/**
 * How much of its disk limit an application uses, as the Node last measured
 * it - shown whether it runs or not, because "stopped for being over its
 * limit" is exactly when somebody needs to see this.
 */
export function DiskUsageBar({ applicationId }: { applicationId: string }) {
  const { t, i18n } = useTranslation();
  const [usage, setUsage] = useState<DiskUsage | null | undefined>(undefined);

  useEffect(() => {
    let cancelled = false;
    const load = () =>
      getApplicationDiskUsage(applicationId)
        .then((result) => {
          if (!cancelled) setUsage(result);
        })
        .catch((err) => console.warn("couldn't read the disk check", err));
    void load();
    const timer = window.setInterval(() => void load(), RELOAD_MS);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [applicationId]);

  if (usage === undefined) return null;
  if (usage === null) return <p className="disk-usage-note">{t("diskUsage.pending")}</p>;

  const percent = usage.limitBytes > 0 ? Math.min(100, (usage.usedBytes / usage.limitBytes) * 100) : 0;
  const over = usage.usedBytes > usage.limitBytes;
  const tone = over ? "disk-usage-over" : percent >= 90 ? "disk-usage-near" : "";

  return (
    <div className={`disk-usage ${tone}`.trim()}>
      <div className="disk-usage-head">
        <span className="disk-usage-label">{t("diskUsage.label")}</span>
        <span className="disk-usage-value">
          {t("diskUsage.value", { used: formatBytes(usage.usedBytes), limit: formatBytes(usage.limitBytes) })}
        </span>
      </div>
      <div className="disk-usage-track" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(percent)}>
        <div className="disk-usage-fill" style={{ width: `${percent}%` }} />
      </div>
      <p className="disk-usage-note">
        {over && <Icon name="alert-triangle" size={12} />}
        {over
          ? t("diskUsage.over")
          : t("diskUsage.checked", { time: new Date(usage.checkedAt).toLocaleTimeString(i18n.language, { hour: "2-digit", minute: "2-digit" }) })}
      </p>
    </div>
  );
}
