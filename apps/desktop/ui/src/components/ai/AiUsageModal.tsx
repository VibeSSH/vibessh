import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { Dialog } from "@/components/ui/Dialog";
import type { AiQuota } from "@/types/ai";
import "./AiUsageModal.css";

interface AiUsageModalProps {
  quota: AiQuota;
  onClose: () => void;
}

/**
 * Today's use of the included model.
 *
 * **Why both numbers are here.** The allowance is counted in questions
 * because that is the only unit a user can be told in one sentence - but a
 * question's cost is not constant, and a Diagnose turn carrying a Node's
 * configuration and forty log lines is worth many times a one-line
 * question. Showing the characters sent alongside the questions asked is
 * what lets somebody notice they have eighteen of twenty left and have
 * already sent most of the day's actual cost. Neither number alone tells
 * that story.
 *
 * **The reset is shown in local time.** It is stored and enforced at
 * midnight UTC, which is the right choice for a limit shared across a team
 * in several timezones, and the wrong thing to show a person - "resets at
 * 01:00" is actionable where "resets at midnight UTC" is arithmetic.
 */
export function AiUsageModal({ quota, onClose }: AiUsageModalProps) {
  const { t } = useTranslation();

  const remaining = Math.max(quota.limit - quota.used, 0);
  const usedFraction = quota.limit > 0 ? Math.min(quota.used / quota.limit, 1) : 0;
  const exhausted = remaining === 0;

  const resets = new Date(quota.resetsAt);
  const resetsValid = !Number.isNaN(resets.getTime());
  const hoursLeft = resetsValid ? Math.max(Math.round((resets.getTime() - Date.now()) / 3_600_000), 0) : 0;

  return (
    <Dialog open onClose={onClose} size="sm" title={t("aiUsage.title")}>
      <div className="modal-body">
        <div className="ai-usage-headline">
          <span className={`ai-usage-remaining ${exhausted ? "ai-usage-remaining-spent" : ""}`}>{remaining}</span>
          <span className="ai-usage-of">{t("aiUsage.ofLimit", { limit: quota.limit })}</span>
        </div>

        {/* `aria-hidden` because the bar is a second rendering of the
              sentence above it, not new information - a screen reader
              announcing both would read the same fact twice. */}
        <div className="ai-usage-bar" aria-hidden="true">
          <div className={`ai-usage-bar-fill ${exhausted ? "ai-usage-bar-fill-spent" : ""}`} style={{ width: `${Math.round(usedFraction * 100)}%` }} />
        </div>

        <dl className="ai-usage-facts">
          <div className="ai-usage-fact">
            <dt>{t("aiUsage.asked")}</dt>
            <dd>{quota.used}</dd>
          </div>
          <div className="ai-usage-fact">
            <dt>{t("aiUsage.context")}</dt>
            <dd>{t("aiUsage.contextValue", { thousands: Math.round(quota.promptChars / 1000) })}</dd>
          </div>
          <div className="ai-usage-fact">
            <dt>{t("aiUsage.resets")}</dt>
            <dd>
              {resetsValid
                ? t("aiUsage.resetsValue", {
                    time: resets.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" }),
                    hours: hoursLeft,
                  })
                : t("aiUsage.resetsUnknown")}
            </dd>
          </div>
        </dl>

        <p className="form-note">{exhausted ? t("aiUsage.exhaustedNote") : t("aiUsage.note")}</p>

        <div className="form-actions">
          <Button variant="secondary" onClick={onClose}>
            {t("common.close")}
          </Button>
        </div>
      </div>
    </Dialog>
  );
}
