import { useEffect, useRef, useState } from "react";
import { useLocation } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { getCurrentWindow, type Window } from "@tauri-apps/api/window";
import { EarlyAccessBadge } from "@/components/layout/EarlyAccessBadge";
import { Icon } from "@/components/ui/Icon";
import { Tooltip } from "@/components/ui/Tooltip";
import { useRipple } from "@/hooks/useRipple";
import { sidebarGroups } from "@/config/navigation";
import { cloudLogout } from "@/services/cloudService";
import { useAuthModalStore } from "@/stores/authModalStore";
import { useAuthStore } from "@/stores/authStore";
import { useToastStore, toastSuccess } from "@/stores/toastStore";
import { formatRelativeTime } from "@/utils/formatRelativeTime";
import { UpdateButton } from "./UpdateButton";
import "./TopBar.css";

/**
 * The single compact top bar. It replaces both the old title bar and the
 * far-left icon rail: on the left, a breadcrumb-style context for the current
 * view; on the right, the global actions (update, notifications, account) that
 * used to live at the bottom of the rail, then the frameless window controls.
 *
 * `getCurrentWindow()` reads a Tauri-only internal, so it is resolved lazily
 * and cached - a missing runtime degrades the window buttons to inert rather
 * than crashing the app on import (as it would in the browser preview).
 */
let cachedWindow: Window | null | undefined;
function currentWindow(): Window | null {
  if (cachedWindow !== undefined) return cachedWindow;
  try {
    cachedWindow = getCurrentWindow();
  } catch {
    cachedWindow = null;
  }
  return cachedWindow;
}

const TONE_ICON: Record<string, string> = { success: "check", error: "x", info: "activity" };

/** The label of the view the current path belongs to, for the breadcrumb. */
function useViewLabel(): string {
  const { t } = useTranslation();
  const { pathname } = useLocation();
  for (const group of sidebarGroups) {
    for (const item of group.items) {
      const base = item.path === "/" ? "/" : item.path;
      if (item.path === "/" ? pathname === "/" : pathname === base || pathname.startsWith(base + "/")) {
        return t(item.labelKey);
      }
    }
  }
  return t("nav.dashboard");
}

function useOutsideClose(open: boolean, onClose: () => void) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    function handle(event: MouseEvent) {
      if (ref.current && !ref.current.contains(event.target as Node)) onClose();
    }
    document.addEventListener("mousedown", handle);
    return () => document.removeEventListener("mousedown", handle);
  }, [open, onClose]);
  return ref;
}

function TopBarIconButton({ icon, label, onClick, badge }: { icon: string; label: string; onClick?: () => void; badge?: number }) {
  const { createRipple, rippleEls } = useRipple();
  return (
    <Tooltip label={label} placement="bottom">
      <button className="topbar-action ripple-host" onPointerDown={createRipple} onClick={onClick} aria-label={label}>
        {rippleEls}
        <Icon name={icon} size={16} />
        {badge ? <span className="topbar-action-badge">{badge > 9 ? "9+" : badge}</span> : null}
      </button>
    </Tooltip>
  );
}

function NotificationsMenu() {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const history = useToastStore((s) => s.history);
  const clearHistory = useToastStore((s) => s.clearHistory);
  const ref = useOutsideClose(open, () => setOpen(false));

  return (
    <div className="topbar-menu-anchor" ref={ref}>
      <TopBarIconButton icon="bell" label={t("rail.notifications")} onClick={() => setOpen((o) => !o)} badge={history.length} />
      {open && (
        <div className="topbar-popover">
          <div className="topbar-popover-header">
            <span>{t("rail.notifications")}</span>
            {history.length > 0 && (
              <button className="topbar-popover-clear" onClick={clearHistory}>
                {t("rail.notificationsClear")}
              </button>
            )}
          </div>
          {history.length === 0 ? (
            <p className="topbar-popover-empty">{t("rail.notificationsEmpty")}</p>
          ) : (
            <ul className="topbar-popover-list">
              {history.map((entry) => (
                <li key={entry.id} className="topbar-notification">
                  <Icon name={TONE_ICON[entry.tone]} size={13} className={`topbar-notification-icon topbar-notification-icon-${entry.tone}`} />
                  <span className="topbar-notification-message">{entry.message}</span>
                  <span className="topbar-notification-time">{formatRelativeTime(entry.at, t)}</span>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </div>
  );
}

function AccountMenu() {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const user = useAuthStore((s) => s.user);
  const setUser = useAuthStore((s) => s.setUser);
  const openAuthModal = useAuthModalStore((s) => s.open);
  const ref = useOutsideClose(open, () => setOpen(false));

  async function handleLogout() {
    setOpen(false);
    try {
      await cloudLogout();
    } catch {
      // Local session is cleared regardless - logout is best-effort against the backend.
    }
    setUser(null);
    toastSuccess(t("auth.loggedOutToast"));
  }

  return (
    <div className="topbar-menu-anchor" ref={ref}>
      <TopBarIconButton icon="user" label={t("rail.account")} onClick={() => setOpen((o) => !o)} />
      {open && (
        <div className="topbar-popover topbar-popover-account">
          <div className="topbar-popover-header">
            <span>{t("rail.account")}</span>
          </div>
          {user ? (
            <>
              <div className="topbar-account-info">
                <p className="topbar-account-name">{user.displayName}</p>
                <p className="topbar-account-email">{user.email}</p>
              </div>
              <button className="topbar-account-action" onClick={handleLogout}>
                <Icon name="x" size={13} />
                {t("rail.signOut")}
              </button>
            </>
          ) : (
            <button
              className="topbar-account-action"
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

export function TopBar() {
  const { t } = useTranslation();
  const viewLabel = useViewLabel();

  function handleMouseDown(event: React.MouseEvent) {
    if (event.button !== 0) return;
    const target = event.target as HTMLElement;
    if (!target.closest('button, a, input, [role="button"]')) {
      currentWindow()
        ?.startDragging()
        .catch((err) => console.error("startDragging failed:", err));
    }
  }

  return (
    <div className="topbar" onMouseDown={handleMouseDown}>
      <div className="topbar-context">
        <img src="/vibessh-mark.svg" alt="" className="topbar-mark" />
        <span className="topbar-crumb-root">VibeSSH</span>
        <Icon name="chevron-right" size={13} className="topbar-crumb-sep" />
        <span className="topbar-crumb-current">{viewLabel}</span>
      </div>

      <div className="topbar-right">
        <EarlyAccessBadge />
        <div className="topbar-actions">
          <UpdateButton />
          <NotificationsMenu />
          <AccountMenu />
        </div>
        <div className="topbar-window">
          <Tooltip label={t("titlebar.minimize")} placement="bottom">
            <button
              className="topbar-win-btn"
              onClick={() => currentWindow()?.minimize().catch((err) => console.error("minimize failed:", err))}
              aria-label={t("titlebar.minimize")}
            >
              <Icon name="minus" size={16} />
            </button>
          </Tooltip>
          <Tooltip label={t("titlebar.maximize")} placement="bottom">
            <button
              className="topbar-win-btn"
              onClick={() => currentWindow()?.toggleMaximize().catch((err) => console.error("toggleMaximize failed:", err))}
              aria-label={t("titlebar.maximize")}
            >
              <Icon name="square" size={12} />
            </button>
          </Tooltip>
          <Tooltip label={t("titlebar.close")} placement="bottom">
            <button
              className="topbar-win-btn topbar-win-close"
              onClick={() => currentWindow()?.close().catch((err) => console.error("close failed:", err))}
              aria-label={t("titlebar.close")}
            >
              <Icon name="x" size={16} />
            </button>
          </Tooltip>
        </div>
      </div>
    </div>
  );
}
