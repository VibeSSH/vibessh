import { getCurrentWindow, type Window } from "@tauri-apps/api/window";
import { useTranslation } from "react-i18next";
import { Icon } from "@/components/ui/Icon";
import { useRipple } from "@/hooks/useRipple";
import "./TitleBar.css";

/**
 * `getCurrentWindow()` reads `window.__TAURI_INTERNALS__.metadata`, which
 * only exists inside a real Tauri webview - calling it at module scope (as
 * Voltius's own TitleBar.tsx does) throws immediately when this app runs in
 * a plain browser during frontend development (`npm run dev` outside
 * `tauri dev`, or this project's browser-preview-based UI verification).
 * Resolved lazily and cached instead, so a missing Tauri runtime degrades
 * to inert buttons rather than crashing the whole app on import.
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

interface TitleBarBtnProps {
  icon: string;
  size: number;
  onClick: () => void;
  className?: string;
  ariaLabel: string;
}

/** Matches Voltius's own TitleBarBtn - every one of their window controls also gets the ripple. */
function TitleBarBtn({ icon, size, onClick, className, ariaLabel }: TitleBarBtnProps) {
  const { createRipple, rippleEls } = useRipple();
  return (
    <button className={`titlebar-btn ripple-host ${className ?? ""}`} onPointerDown={createRipple} onClick={onClick} aria-label={ariaLabel}>
      {rippleEls}
      <Icon name={icon} size={size} />
    </button>
  );
}

/**
 * A custom-drawn titlebar replacing the OS chrome (tauri.conf.json sets
 * `decorations: false`), matching Voltius's own frameless-window setup
 * (voltius/src-tauri/tauri.conf.json, voltius/src/components/layout/
 * TitleBar.tsx). Dragging is wired by hand rather than the `data-tauri-
 * drag-region` HTML attribute, same as Voltius: a mousedown anywhere on the
 * bar that isn't a button/input/link starts the OS window-move gesture via
 * `appWindow.startDragging()`.
 *
 * All four window methods here (minimize/toggleMaximize/close/
 * startDragging) need their own explicit permission in capabilities/
 * default.json - Tauri 2's `core:default` window permission set is
 * read-only (is_minimized, is_maximized, etc.), not the commands that
 * actually change window state. Missing that was a real bug: the window
 * genuinely couldn't be moved, minimized, or maximized in the real app,
 * silently (a rejected permission check just rejects the promise; nothing
 * surfaced in the UI, and the browser-preview verification this project
 * otherwise leans on couldn't have caught it either, since currentWindow()
 * resolves to null there before any permission check would even run).
 * .catch(console.error) below at least makes a *future* permission gap
 * visible in devtools instead of a silently inert button.
 */
export function TitleBar() {
  const { t } = useTranslation();

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
    <div className="titlebar" onMouseDown={handleMouseDown}>
      {/* No app-name label here - NavBar's own brand mark sits directly
          underneath and showing "VibeSSH" in both looked like broken,
          overlapping text rather than two separate rows. */}
      <div className="titlebar-controls">
        <TitleBarBtn
          icon="minus"
          size={16}
          onClick={() => currentWindow()?.minimize().catch((err) => console.error("minimize failed:", err))}
          ariaLabel={t("titlebar.minimize")}
        />
        <TitleBarBtn
          icon="square"
          size={12}
          onClick={() => currentWindow()?.toggleMaximize().catch((err) => console.error("toggleMaximize failed:", err))}
          ariaLabel={t("titlebar.maximize")}
        />
        <TitleBarBtn
          icon="x"
          size={16}
          onClick={() => currentWindow()?.close().catch((err) => console.error("close failed:", err))}
          className="titlebar-btn-close"
          ariaLabel={t("titlebar.close")}
        />
      </div>
    </div>
  );
}
