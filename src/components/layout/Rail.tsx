import { useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { Icon } from "@/components/ui/Icon";
import { useServerModalStore } from "@/stores/serverModalStore";
import { useToastStore } from "@/stores/toastStore";
import "./Rail.css";

function formatRelativeTime(at: number): string {
  const seconds = Math.max(0, Math.floor((Date.now() - at) / 1000));
  if (seconds < 5) return "just now";
  if (seconds < 60) return `${seconds}s ago`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  return `${Math.floor(hours / 24)}d ago`;
}

const TONE_ICON: Record<string, string> = { success: "check", error: "x", info: "activity" };

function NotificationBell() {
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
      <button className="rail-btn" onClick={() => setOpen((o) => !o)} aria-label="Notifications" title="Notifications">
        <Icon name="bell" size={18} />
      </button>
      {open && (
        <div className="rail-popover rail-popover-right">
          <div className="rail-popover-header">
            <span>Notifications</span>
            {history.length > 0 && (
              <button className="rail-popover-clear" onClick={clearHistory}>
                Clear
              </button>
            )}
          </div>
          {history.length === 0 ? (
            <p className="rail-popover-empty">Nothing yet - actions you take show up here.</p>
          ) : (
            <ul className="rail-popover-list">
              {history.map((entry) => (
                <li key={entry.id} className="rail-notification">
                  <Icon name={TONE_ICON[entry.tone]} size={13} className={`rail-notification-icon rail-notification-icon-${entry.tone}`} />
                  <span className="rail-notification-message">{entry.message}</span>
                  <span className="rail-notification-time">{formatRelativeTime(entry.at)}</span>
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
      <button className="rail-btn" onClick={() => setOpen((o) => !o)} aria-label="Account" title="Account">
        <Icon name="user" size={18} />
      </button>
      {open && (
        <div className="rail-popover rail-popover-right rail-popover-bottom">
          <div className="rail-popover-header">
            <span>Account</span>
          </div>
          <p className="rail-popover-empty">
            Cloud accounts aren't built yet - VibeSSH is fully local for now. This is where sign-in will live once that's real.
          </p>
        </div>
      )}
    </div>
  );
}

/**
 * Narrow icon rail on the far left, ported from Voltius's vault-switcher
 * rail (voltius/src/components/layout/VaultSidebar.tsx) - Voltius uses the
 * top of it to switch vaults, which VibeSSH has no equivalent of, so only
 * the bottom utility icons (account/notifications/settings) and a quick-add
 * button carry over, repurposed for what VibeSSH actually has.
 */
export function Rail() {
  const navigate = useNavigate();
  const openForCreate = useServerModalStore((s) => s.openForCreate);

  return (
    <div className="rail">
      <button className="rail-btn rail-add-btn" onClick={openForCreate} aria-label="Add server" title="Add server">
        <Icon name="plus" size={18} />
      </button>

      <div className="rail-spacer" />

      <AccountButton />
      <NotificationBell />
      <button className="rail-btn" onClick={() => navigate("/settings")} aria-label="Settings" title="Settings">
        <Icon name="settings" size={18} />
      </button>
    </div>
  );
}
