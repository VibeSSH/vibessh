import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";
import { HostAddress } from "@/components/ui/HostAddress";
import { useCachedServerMetrics, useServerMetricsStore } from "@/stores/serverMetricsStore";
import { formatBytesOf } from "@/utils/formatBytes";
import { Icon } from "@/components/ui/Icon";
import { Tooltip } from "@/components/ui/Tooltip";
import { useRipple } from "@/hooks/useRipple";
import { cloudLogout } from "@/services/cloudService";
import { useAuthModalStore } from "@/stores/authModalStore";
import { useAuthStore } from "@/stores/authStore";
import { usePingStore } from "@/stores/pingStore";
import { useServerModalStore } from "@/stores/serverModalStore";
import { useServersStore, type ManagedServer } from "@/stores/serversStore";
import { toastSuccess } from "@/stores/toastStore";
import { useToastStore } from "@/stores/toastStore";
import { formatRelativeTime } from "@/utils/formatRelativeTime";
import "./Rail.css";
import { StatusDot } from "@/components/ui/StatusDot";
import { NodeIcon } from "@/components/servers/NodeIcon";

interface RailButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  icon: string;
  iconSize?: number;
}

function RailButton({ icon, iconSize = 18, className, title, ...rest }: RailButtonProps) {
  const { createRipple, rippleEls } = useRipple();
  const button = (
    <button className={`rail-btn ripple-host ${className ?? ""}`} onPointerDown={createRipple} {...rest}>
      {rippleEls}
      <Icon name={icon} size={iconSize} />
    </button>
  );
  return title ? (
    <Tooltip label={String(title)} placement="right">
      {button}
    </Tooltip>
  ) : (
    button
  );
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
        <div className="rail-popover rail-popover-right rail-popover-bottom">
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
  const user = useAuthStore((s) => s.user);
  const setUser = useAuthStore((s) => s.setUser);
  const openAuthModal = useAuthModalStore((s) => s.open);

  useEffect(() => {
    if (!open) return;
    function handlePointerDown(event: MouseEvent) {
      if (anchorRef.current && !anchorRef.current.contains(event.target as Node)) setOpen(false);
    }
    document.addEventListener("mousedown", handlePointerDown);
    return () => document.removeEventListener("mousedown", handlePointerDown);
  }, [open]);

  async function handleLogout() {
    setOpen(false);
    try {
      await cloudLogout();
    } catch {
      // Local session is cleared below regardless - see cloud_service::logout's own comment on why logout is best-effort against the backend.
    }
    setUser(null);
    toastSuccess(t("auth.loggedOutToast"));
  }

  return (
    <div className="rail-popover-anchor" ref={anchorRef}>
      <RailButton icon="user" onClick={() => setOpen((o) => !o)} aria-label={t("rail.account")} title={t("rail.account")} />
      {open && (
        <div className="rail-popover rail-popover-right rail-popover-bottom">
          <div className="rail-popover-header">
            <span>{t("rail.account")}</span>
          </div>
          {user ? (
            <>
              <div className="rail-account-info">
                <p className="rail-account-name">{user.displayName}</p>
                <p className="rail-account-email">{user.email}</p>
              </div>
              <button
                className="rail-account-action"
                onClick={handleLogout}
              >
                <Icon name="x" size={13} />
                {t("rail.signOut")}
              </button>
            </>
          ) : (
            <button
              className="rail-account-action"
              onClick={() => {
                setOpen(false);
                openAuthModal();
              }}
            >
              <Icon name="user" size={13} />
              {t("rail.signIn")}
            </button>
          )}
        </div>
      )}
    </div>
  );
}

/** Missing for agent-paired servers (not yet Etap-2-persisted, see serversStore's own note) - treated as "just added right now" for sort purposes, which is exactly what pairing one during this session actually means. */
function sortableTimestamp(server: ManagedServer): number {
  return server.createdAt ? new Date(server.createdAt).getTime() : Date.now();
}

/**
 * A custom-styled popover rather than the native `title` tooltip this used
 * to be - two reasons: the OS tooltip rendered as a plain light-mode box
 * that clashed with the rest of the dark UI, and (more importantly) it put
 * the server's real host/IP in plain text right next to the mouse cursor,
 * which is exactly the spot that ends up in a casual screenshot (Discord
 * screen share, a bug report, etc.). The host is masked by default here and
 * only shown after an explicit click on the eye icon, which resets on the
 * next hover.
 */
function RailInstanceButton({ server }: { server: ManagedServer }) {
  const { t } = useTranslation();
  const { createRipple, rippleEls } = useRipple();
  const navigate = useNavigate();
  const latencyMs = usePingStore((s) => s.latencies[server.id]);
  const isAgent = server.connectionMode === "agent";
  // What the machine has, not only whether it answers. Read from the
  // shared cache, so a hover usually costs nothing at all - see
  // `serverMetricsStore` for why this is not a fetch per hover.
  const metrics = useCachedServerMetrics(server.id);
  const ensureMetrics = useServerMetricsStore((s) => s.ensure);
  const btnRef = useRef<HTMLButtonElement>(null);
  const closeTimer = useRef<number | undefined>(undefined);
  const [tooltipPos, setTooltipPos] = useState<{ top: number; left: number } | null>(null);

  /**
   * The tooltip is rendered through a portal (see below), so it isn't a DOM
   * descendant of this anchor - moving the mouse from the button into the
   * tooltip briefly leaves the anchor's own bounding box, which would fire
   * onMouseLeave and close the tooltip before the pointer ever reaches the
   * reveal-host button inside it. Debouncing the close (and having the
   * tooltip itself cancel it on enter, below) bridges that gap the way any
   * portal-based popover has to.
   */
  function handleEnter() {
    window.clearTimeout(closeTimer.current);
    const rect = btnRef.current?.getBoundingClientRect();
    if (rect) setTooltipPos({ top: rect.top + rect.height / 2, left: rect.right + 10 });
    // Skipped only where the attempt is known to be pointless: agent mode
    // has no metrics command, and a Node already known to be offline will
    // not answer. "Unknown" is not "offline" - it means nothing has checked
    // yet, which on a freshly opened app is exactly when somebody hovers,
    // and refusing to try there left the card permanently blank.
    if (!isAgent && server.status !== "offline") ensureMetrics(server.id);
  }
  function handleLeave() {
    closeTimer.current = window.setTimeout(() => {
      setTooltipPos(null);
    }, 120);
  }

  useEffect(() => () => window.clearTimeout(closeTimer.current), []);

  return (
    <div className="rail-instance-anchor" onMouseEnter={handleEnter} onMouseLeave={handleLeave}>
      <button
        ref={btnRef}
        className="rail-btn rail-instance-btn ripple-host"
        onPointerDown={createRipple}
        onClick={() => navigate(isAgent ? "/servers" : `/terminal/${server.id}`)}
        aria-label={server.name}
      >
        {rippleEls}
        {/* The icon is why this feature exists: at four or five nodes the
            rail's generic glyphs are indistinguishable, and the name only
            appears on hover. A custom image replaces the glyph entirely
            rather than sitting beside it - there is no room for both. */}
        <NodeIcon server={server} size={16} />
        {/* No room for the word on a 40px button, so this is the one place
            the dot carries the status on its own - as a shape plus an
            accessible name, not as a colour. */}
        <StatusDot status={server.status} className="rail-instance-status-slot" />
      </button>

      {tooltipPos &&
        createPortal(
          <div
            className="rail-instance-tooltip"
            style={{ top: tooltipPos.top, left: tooltipPos.left }}
            onMouseEnter={handleEnter}
            onMouseLeave={handleLeave}
          >
            <div className="rail-instance-tooltip-header">
              <span className="rail-instance-tooltip-name" title={server.name}>{server.name}</span>
              <span className="rail-instance-tooltip-status">
                <StatusDot status={server.status} withLabel />
              </span>
            </div>
            <HostAddress value={server.host} className="rail-instance-tooltip-host" />
            {!isAgent && server.status === "online" && typeof latencyMs === "number" && (
              <div className="rail-instance-tooltip-latency">{latencyMs} ms</div>
            )}
            {/* Absent until a reading exists, rather than shown as zeros: a
                machine reporting "0 / 0 GB" looks broken, and "nothing yet"
                is the honest state for the first second of a hover. */}
            {metrics && (
              <dl className="rail-instance-tooltip-facts">
                {/* First, because it is the fact that frames the other two:
                    how much memory is a lot depends on what is running. */}
                {metrics.osName && (
                  <>
                    <dt>{t("rail.system")}</dt>
                    <dd className="rail-instance-tooltip-os" title={metrics.osName}>
                      {metrics.osName}
                    </dd>
                  </>
                )}
                <dt>{t("rail.ram")}</dt>
                <dd>{formatBytesOf(metrics.ramUsedBytes, metrics.ramTotalBytes)}</dd>
                <dt>{t("rail.disk")}</dt>
                <dd>{formatBytesOf(metrics.diskUsedBytes, metrics.diskTotalBytes)}</dd>
              </dl>
            )}
          </div>,
          document.body,
        )}
    </div>
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
