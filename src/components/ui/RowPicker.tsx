import { useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Badge } from "./Badge";
import { Icon } from "./Icon";
import { useClickOutside } from "@/hooks/useClickOutside";
import "./RowPicker.css";

export interface RowPickerOption {
  id: string;
  icon?: string;
  name: string;
  meta?: string;
  status?: { tone: "success" | "danger" | "warning" | "neutral"; label: string };
  extra?: string;
}

interface RowPickerProps {
  options: RowPickerOption[];
  value: string | null;
  onChange: (id: string) => void;
  placeholder: string;
  label?: string;
  emptyNote?: string;
  disabled?: boolean;
}

/** Every "pick a server" `RowPicker` in the app wants the same row shape -
 * name, host, and a real connection-status badge - so this lives here
 * once rather than every call site (VibeNetwork's node pickers, the
 * migrate-application target picker, DatabaseHosts' linked-server picker)
 * re-deriving the tone/label mapping itself. Takes a minimal duck-typed
 * shape rather than importing `ManagedServer` directly, so this UI
 * primitive doesn't depend on a specific store's type. */
export function serverRowPickerOption(
  server: { id: string; name: string; host: string; status: "online" | "offline" | "connecting" | "unknown" },
  t: (key: string) => string,
): RowPickerOption {
  const tone: "success" | "danger" | "warning" | "neutral" =
    server.status === "online" ? "success" : server.status === "offline" ? "danger" : server.status === "connecting" ? "warning" : "neutral";
  const label = t(`rail.status${server.status.charAt(0).toUpperCase()}${server.status.slice(1)}`);
  return { id: server.id, name: server.name, meta: server.host, status: { tone, label } };
}

const VIEWPORT_MARGIN = 8;
const PANEL_GAP = 4;
const PANEL_MAX_HEIGHT = 320;

/**
 * Replaces a bare `<select>` for "pick one server/app/target" flows - a
 * native `<option>` can't carry a host, a status dot, or a latency reading,
 * so this renders real rows (extends the same `.server-list-item` row
 * shape used everywhere else in the app) in a portal-positioned panel that
 * sizes to its content instead of a fixed height.
 */
export function RowPicker({ options, value, onChange, placeholder, label, emptyNote, disabled }: RowPickerProps) {
  const [open, setOpen] = useState(false);
  const [coords, setCoords] = useState<{ top: number; left: number; width: number } | null>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);

  useClickOutside(panelRef, () => setOpen(false), open);

  useLayoutEffect(() => {
    if (!open || !triggerRef.current) return;
    function reposition() {
      const anchor = triggerRef.current!.getBoundingClientRect();
      const panelRect = panelRef.current?.getBoundingClientRect();
      const panelHeight = panelRect?.height ?? 0;
      // The panel is free to grow wider than the trigger to fit a row's
      // icon/name/status badge without clipping (see `.row-picker-panel`'s
      // own min-width-not-width CSS) - clamp against its own actual
      // rendered width, not the (possibly much narrower) trigger's.
      const panelWidth = panelRect?.width ?? anchor.width;
      let top = anchor.bottom + PANEL_GAP;
      if (top + panelHeight > window.innerHeight - VIEWPORT_MARGIN) {
        top = Math.max(VIEWPORT_MARGIN, anchor.top - panelHeight - PANEL_GAP);
      }
      const left = Math.min(anchor.left, window.innerWidth - panelWidth - VIEWPORT_MARGIN);
      setCoords({ top, left: Math.max(VIEWPORT_MARGIN, left), width: anchor.width });
    }
    reposition();
    window.addEventListener("resize", reposition);
    window.addEventListener("scroll", reposition, true);
    return () => {
      window.removeEventListener("resize", reposition);
      window.removeEventListener("scroll", reposition, true);
    };
  }, [open]);

  const selected = options.find((o) => o.id === value) ?? null;

  const trigger = (
    <button
      ref={triggerRef}
      type="button"
      className="form-input row-picker-trigger"
      aria-haspopup="listbox"
      aria-expanded={open}
      disabled={disabled}
      onClick={() => setOpen((o) => !o)}
    >
      <span className={`row-picker-trigger-label ${selected ? "" : "row-picker-trigger-placeholder"}`}>{selected ? selected.name : placeholder}</span>
      <Icon name="chevron-down" size={14} />
    </button>
  );

  return (
    <>
      {label ? (
        <label className="form-field">
          <span className="form-label">{label}</span>
          {trigger}
        </label>
      ) : (
        trigger
      )}

      {open &&
        createPortal(
          <div
            ref={panelRef}
            role="listbox"
            className="row-picker-panel anim-scale-in"
            style={coords ? { top: coords.top, left: coords.left, minWidth: coords.width, maxHeight: PANEL_MAX_HEIGHT } : { top: -9999, left: -9999 }}
            /* This panel is portal-rendered outside the DOM subtree of
               whatever form/modal opened it - a click anywhere inside it
               (including empty row padding, not just a row's own button)
               must never be mistaken for "clicked outside" by a parent
               modal's own backdrop-close handler. Same defensive
               stopPropagation `.modal-panel` itself already uses. */
            onMouseDown={(e) => e.stopPropagation()}
            onClick={(e) => e.stopPropagation()}
          >
            {options.length === 0 ? (
              <p className="row-picker-empty">{emptyNote}</p>
            ) : (
              <ul className="server-list row-picker-list">
                {options.map((option) => {
                  const isSelected = option.id === value;
                  return (
                    <li key={option.id} className="server-list-item row-picker-option-row">
                      <button
                        type="button"
                        role="option"
                        aria-selected={isSelected}
                        className={`row-picker-option ${isSelected ? "row-picker-option-selected" : ""}`}
                        onClick={() => {
                          onChange(option.id);
                          setOpen(false);
                        }}
                      >
                        <div className="server-list-icon">
                          <Icon name={option.icon ?? "server"} size={16} />
                        </div>
                        <div className="server-list-main">
                          <span className="server-list-name">{option.name}</span>
                          {option.meta && <span className="server-list-host">{option.meta}</span>}
                        </div>
                        {option.status && <Badge tone={option.status.tone}>{option.status.label}</Badge>}
                        {option.extra && <span className="row-picker-option-extra">{option.extra}</span>}
                        {isSelected && <Icon name="check" size={16} />}
                      </button>
                    </li>
                  );
                })}
              </ul>
            )}
          </div>,
          document.body,
        )}
    </>
  );
}
