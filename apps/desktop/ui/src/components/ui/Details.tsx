import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Icon } from "./Icon";
import "./Details.css";

interface DetailsProps {
  /** Summary label; defaults to the localized "Details" ("Szczegóły"). */
  summary?: string;
  children: ReactNode;
  className?: string;
  /** Start expanded. Off by default - the whole point is to hide long copy. */
  defaultOpen?: boolean;
}

/**
 * A small "Details"/"Szczegóły" disclosure for the long, always-there
 * technical notes that otherwise crowd forms and cards. Built on the native
 * <details>/<summary> so it needs no state, is keyboard-operable for free,
 * and the marker below rotates with [open]. Text stays 13px muted, in line
 * with the rest of the form copy.
 */
export function Details({ summary, children, className, defaultOpen = false }: DetailsProps) {
  const { t } = useTranslation();
  const classes = ["details-disclosure", className].filter(Boolean).join(" ");
  return (
    <details className={classes} open={defaultOpen}>
      <summary className="details-summary">
        <Icon name="chevron-right" size={13} className="details-chevron" />
        {summary ?? t("common.details")}
      </summary>
      <div className="details-body">{children}</div>
    </details>
  );
}
