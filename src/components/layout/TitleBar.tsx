import { getCurrentWindow, type Window } from "@tauri-apps/api/window";
import { Icon } from "@/components/ui/Icon";
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

/**
 * A custom-drawn titlebar replacing the OS chrome (tauri.conf.json sets
 * `decorations: false`), matching Voltius's own frameless-window setup
 * (voltius/src-tauri/tauri.conf.json, voltius/src/components/layout/
 * TitleBar.tsx). Dragging is wired by hand rather than the `data-tauri-
 * drag-region` HTML attribute, same as Voltius: a mousedown anywhere on the
 * bar that isn't a button/input/link starts the OS window-move gesture via
 * `appWindow.startDragging()`.
 */
export function TitleBar() {
  function handleMouseDown(event: React.MouseEvent) {
    if (event.button !== 0) return;
    const target = event.target as HTMLElement;
    if (!target.closest('button, a, input, [role="button"]')) {
      currentWindow()?.startDragging();
    }
  }

  return (
    <div className="titlebar" onMouseDown={handleMouseDown}>
      {/* No app-name label here - NavBar's own brand mark sits directly
          underneath and showing "VibeSSH" in both looked like broken,
          overlapping text rather than two separate rows. */}
      <div className="titlebar-controls">
        <button className="titlebar-btn" onClick={() => currentWindow()?.minimize()} aria-label="Minimize">
          <Icon name="minus" size={16} />
        </button>
        <button className="titlebar-btn" onClick={() => currentWindow()?.toggleMaximize()} aria-label="Maximize">
          <Icon name="square" size={12} />
        </button>
        <button className="titlebar-btn titlebar-btn-close" onClick={() => currentWindow()?.close()} aria-label="Close">
          <Icon name="x" size={16} />
        </button>
      </div>
    </div>
  );
}
