import { useTranslation } from "react-i18next";
import { Badge } from "@/components/ui/Badge";
import { Card } from "@/components/ui/Card";
import { IconButton } from "@/components/ui/IconButton";
import type { Application, ApplicationStatus } from "@/types/application";
import "./ApplicationCard.css";
import { BlueprintIcon } from "@/components/applications/BlueprintIcon";

export const STATUS_TONE: Record<ApplicationStatus, "neutral" | "success" | "danger" | "warning"> = {
  unknown: "neutral",
  starting: "warning",
  running: "success",
  stopping: "warning",
  stopped: "neutral",
  failed: "danger",
};

interface ApplicationCardProps {
  application: Application;
  serverName?: string;
  busy: boolean;
  /** Absent means this list has no selection - no checkbox is drawn at all. */
  selected?: boolean;
  onSelectedChange?: (selected: boolean) => void;
  onOpen: () => void;
  onStart: () => void;
  onStop: () => void;
  onRestart: () => void;
  onKill: () => void;
  onDelete: () => void;
  /** Somebody else's application, shared with this account: marked as such,
   *  with no delete - it is not this install's to remove - and start/stop
   *  only when the grant allows it. */
  shared?: { canLifecycle: boolean } | null;
}

export function ApplicationCard({
  application,
  serverName,
  busy,
  selected,
  onSelectedChange,
  onOpen,
  onStart,
  onStop,
  onRestart,
  onKill,
  onDelete,
  shared = null,
}: ApplicationCardProps) {
  const { t } = useTranslation();
  const isLocal = !application.serverId;
  const mayLifecycle = !shared || shared.canLifecycle;
  const canStart = mayLifecycle && (application.status === "stopped" || application.status === "failed" || application.status === "unknown");
  const canStopOrRestart = mayLifecycle && (application.status === "running" || application.status === "starting");

  // The header is the selection target when the list offers one, and inert
  // markup when it does not - a card outside that context is exactly what it
  // was. A `button` rather than a div with a handler, so it is reachable by
  // keyboard and announces its state; nothing inside it is interactive, so
  // there is no button nested in a button.
  const header = (
    <>
      <div className="application-card-icon">
          <BlueprintIcon blueprintId={application.blueprintId} size={16} />
        </div>
        <div className="application-card-title-col">
          <p className="application-card-name" title={application.name}>
            {application.name}
          </p>
          <p className="application-card-meta">
            {isLocal ? t("applicationCard.local") : (serverName ?? t("applicationCard.remote"))} · {application.blueprintId}
            {shared && <span className="application-card-shared">{t("sharedApplication.badge")}</span>}
          </p>
        </div>
      <Badge tone={STATUS_TONE[application.status]}>{t(`applicationStatus.${application.status}`)}</Badge>
    </>
  );

  const isSelected = selected ?? false;

  /**
   * A click anywhere on the card selects it - except on a control, which
   * already means something specific.
   *
   * The exclusion is the controls themselves, not the row they sit in: the
   * gaps between those buttons are card, and a card that ignores clicks in
   * some of its own empty space is the "I click and nothing happens" this
   * was meant to fix.
   */
  function handleCardClick(event: React.MouseEvent) {
    if (!onSelectedChange) return;
    if ((event.target as HTMLElement).closest("button, a, input, select, textarea")) return;
    onSelectedChange(!isSelected);
  }

  // Not a `<button>` element: the card contains real buttons, and nesting
  // those inside one is invalid markup that browsers resolve by dropping
  // them out. The role, the tab stop and the key handler give the same
  // behaviour to a keyboard and to a screen reader.
  const selectionProps = onSelectedChange
    ? {
        role: "button" as const,
        tabIndex: 0,
        "aria-pressed": isSelected,
        "aria-label": t("applicationCard.selectAria", { name: application.name }),
        onClick: handleCardClick,
        onKeyDown: (event: React.KeyboardEvent) => {
          // Only when the card itself has focus - Enter inside the action
          // row belongs to whichever button is focused there.
          if (event.target !== event.currentTarget) return;
          if (event.key === "Enter" || event.key === " ") {
            event.preventDefault();
            onSelectedChange(!isSelected);
          }
        },
      }
    : {};

  return (
    <Card
      className={`application-card${onSelectedChange ? " application-card-selectable" : ""}${selected ? " application-card-selected" : ""}`}
      {...selectionProps}
    >
      <div className="application-card-header">{header}</div>

      <div className="application-card-actions">
        <div className="application-card-actions-group">
          {canStart && (
            <IconButton icon="play" size="sm" title={t("applicationCard.startAria", { name: application.name })} onClick={onStart} disabled={busy} />
          )}
          {canStopOrRestart && (
            <>
              <IconButton icon="square" size="sm" title={t("applicationCard.stopAria", { name: application.name })} onClick={onStop} disabled={busy} />
              <IconButton icon="refresh-cw" size="sm" title={t("applicationCard.restartAria", { name: application.name })} onClick={onRestart} disabled={busy} />
              <IconButton icon="power" size="sm" danger title={t("applicationCard.killAria", { name: application.name })} onClick={onKill} disabled={busy} />
            </>
          )}
          {!shared && (
            <IconButton icon="trash" size="sm" danger title={t("applicationCard.deleteAria", { name: application.name })} onClick={onDelete} disabled={busy} />
          )}
        </div>
        <IconButton icon="chevron-right" size="sm" title={t("applicationCard.openAria", { name: application.name })} onClick={onOpen} />
      </div>
    </Card>
  );
}
