import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";
import { Icon } from "@/components/ui/Icon";
import { useRipple } from "@/hooks/useRipple";
import { usePingStore } from "@/stores/pingStore";
import { useServerModalStore } from "@/stores/serverModalStore";
import { useServersStore, type ManagedServer } from "@/stores/serversStore";
import { useToastStore } from "@/stores/toastStore";
import { STATUS_COLOR } from "@/utils/serverStatusColor";
import "./Rail.css";

interface RailButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  icon: string;
  iconSize?: number;
}

function RailButton({ icon, iconSize = 18, className, ...rest }: RailButtonProps) {
  const { createRipple, rippleEls } = useRipple();
  return (
    <button className={`rail-btn ripple-host ${className ?? ""}`} onPointerDown={createRipple} {...rest}>
      {rippleEls}
      <Icon name={icon} size={iconSize} />
    </button>
  );
}

function formatRelativeTime(at: number, t: (key: string, opts?: Record<string, unknown>) => string): string {
  const seconds = Math.max(0, Math.floor((Date.now() - at) / 1000));
  if (seconds < 5) return t("time.justNow");
  if (seconds < 60) return t("time.secondsAgo", { count: seconds });
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return t("time.minutesAgo", { count: minutes });
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return t("time.hoursAgo", { count: hours });
  return t("time.daysAgo", { count: Math.floor(hours / 24) });
}

const TONE_ICON: Record<string, string> = { success: "check", error: "x", info: "activity" };

function NotificationBell() {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const anchorRef = useRef<HTMLDivElement>(null);
  const history = useToastStore((s) => s.history);
  const clearHistory = useToastStore((s) => s.clearHistory);

  useEffect(() => {
    if (!open) return;
    function handlePointerDown(event: MouseEvent) {
      if (anchorRef.current && !anchorRef.current.contains(event.target as Node)) setOpen(false);
    }
    document.addEventListener("mousedown", handlePointerDown);
    return () => document.removeEventListener("mousedown", handlePointerDown);
  }, [open]);

  return (
    <div className="rail-popover-anchor" ref={anchorRef}>
      <RailButton icon="bell" onClick={() => setOpen((o) => !o)} aria-label={t("rail.notifications")} title={t("rail.notifications")} />
      {open && (
        <div className="rail-popover rail-popover-right">
          <div className="rail-popover-header">
            <span>{t("rail.notifications")}</span>
            {history.length > 0 && (
              <button className="rail-popover-clear" onClick={clearHistory}>
                {t("rail.notificationsClear")}
              </button>
            )}
          </div>
          {history.length === 0 ? (
            <p className="rail-popover-empty">{t("rail.notificationsEmpty")}</p>
          ) : (
            <ul className="rail-popover-list">
              {history.map((entry) => (
                <li key={entry.id} className="rail-notification">
                  <Icon name={TONE_ICON[entry.tone]} size={13} className={`rail-notification-icon rail-notification-icon-${entry.tone}`} />
                  <span className="rail-notification-message">{entry.message}</span>
                  <span className="rail-notification-time">{formatRelativeTime(entry.at, t)}</span>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </div>
  );
}

function AccountButton() {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const anchorRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    function handlePointerDown(event: MouseEvent) {
      if (anchorRef.current && !anchorRef.current.contains(event.target as Node)) setOpen(false);
    }
    document.addEventListener("mousedown", handlePointerDown);
    return () => document.removeEventListener("mousedown", handlePointerDown);
  }, [open]);

  return (
    <div className="rail-popover-anchor" ref={anchorRef}>
      <RailButton icon="user" onClick={() => setOpen((o) => !o)} aria-label={t("rail.account")} title={t("rail.account")} />
      {open && (
        <div className="rail-popover rail-popover-right rail-popover-bottom">
          <div className="rail-popover-header">
            <span>{t("rail.account")}</span>
          </div>
          <p className="rail-popover-empty">{t("rail.accountEmpty")}</p>
        </div>
      )}
    </div>
  );
}

/** Missing for agent-paired servers (not yet Etap-2-persisted, see serversStore's own note) - treated as "just added right now" for sort purposes, which is exactly what pairing one during this session actually means. */
function sortableTimestamp(server: ManagedServer): number {
  return server.createdAt ? new Date(server.createdAt).getTime() : Date.now();
}

function RailInstanceButton({ server }: { server: ManagedServer }) {
  const { createRipple, rippleEls } = useRipple();
  const navigate = useNavigate();
  const latencyMs = usePingStore((s) => s.latencies[server.id]);
  const isAgent = server.connectionMode === "agent";
  const statusColor = STATUS_COLOR[server.status];

  const tooltipLines = [server.name, server.host];
  if (!isAgent && server.status === "online" && typeof latencyMs === "number") tooltipLines.push(`${latencyMs} ms`);
  const tooltip = tooltipLines.join("\n");

  return (
    <button
      className="rail-btn rail-instance-btn ripple-host"
      onPointerDown={createRipple}
      onClick={() => navigate(isAgent ? "/servers" : `/terminal/${server.id}`)}
      aria-label={tooltip}
      title={tooltip}
    >
      {rippleEls}
      <Icon name={isAgent ? "zap" : "server"} size={16} />
      <span className="rail-instance-status-dot" style={{ background: statusColor }} />
    </button>
  );
}

/**
 * Narrow icon rail on the far left, ported from Voltius's vault-switcher
 * rail (voltius/src/components/layout/VaultSidebar.tsx) - Voltius uses the
 * top of it to switch vaults, which VibeSSH has no equivalent of, so
 * instead the top of the rail is a quick-access list of added servers
 * (newest first), the way a taskbar lists open windows - click one to jump
 * straight to its terminal. The bottom utility icons (account/
 * notifications) carry over from Voltius as-is. Settings lives in the
 * sidebar's Other group instead of here, so it isn't duplicated between two
 * navigation surfaces.
 */
export function Rail() {
  const { t } = useTranslation();
  const openForCreate = useServerModalStore((s) => s.openForCreate);
  const servers = useServersStore((s) => s.servers);
  const sortedServers = [...servers].sort((a, b) => sortableTimestamp(b) - sortableTimestamp(a));

  return (
    <div className="rail">
      <div className="rail-instances">
        {sortedServers.map((server) => (
          <RailInstanceButton key={server.id} server={server} />
        ))}
      </div>

      <RailButton icon="plus" className="rail-add-btn" onClick={openForCreate} aria-label={t("rail.addServer")} title={t("rail.addServer")} />

      <div className="rail-spacer" />

      <AccountButton />
      <NotificationBell />
    </div>
  );
}
