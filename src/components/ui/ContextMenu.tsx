import { useEffect, useLayoutEffect, useRef, useState, type MouseEvent as ReactMouseEvent } from "react";
import { createPortal } from "react-dom";
import { Icon } from "./Icon";
import { useClickOutside } from "@/hooks/useClickOutside";
import "./OverflowMenu.css";

export interface ContextMenuItem {
  label: string;
  icon?: string;
  onClick: () => void;
  danger?: boolean;
  disabled?: boolean;
}

interface ContextMenuState {
  x: number;
  y: number;
  items: ContextMenuItem[];
}

const VIEWPORT_MARGIN = 8;

/**
 * A right-click menu anchored to the click position, not to a trigger
 * element - unlike `OverflowMenu` (a "..." button that owns its own open
 * state), any number of rows share ONE of these: call `open(event, items)`
 * from each row's `onContextMenu`, render `element` once for the whole
 * list/page. Reuses `OverflowMenu`'s own `.overflow-menu`/
 * `.overflow-menu-item` styling rather than a second visual language for
 * what's the same kind of floating action list either way.
 */
export function useContextMenu() {
  const [state, setState] = useState<ContextMenuState | null>(null);
  const [coords, setCoords] = useState<{ top: number; left: number } | null>(null);
  const menuRef = useRef<HTMLDivElement>(null);

  useClickOutside(menuRef, () => setState(null), state !== null);

  useEffect(() => {
    if (!state) return;
    function handleKeyDown(e: KeyboardEvent) {
      if (e.key === "Escape") setState(null);
    }
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [state]);

  useLayoutEffect(() => {
    if (!state) return;
    const menuRect = menuRef.current?.getBoundingClientRect() ?? { width: 0, height: 0 };
    const left = Math.min(Math.max(VIEWPORT_MARGIN, state.x), window.innerWidth - menuRect.width - VIEWPORT_MARGIN);
    const top = Math.min(Math.max(VIEWPORT_MARGIN, state.y), window.innerHeight - menuRect.height - VIEWPORT_MARGIN);
    setCoords({ top, left });
  }, [state]);

  function open(event: ReactMouseEvent, items: ContextMenuItem[]) {
    event.preventDefault();
    event.stopPropagation();
    setState({ x: event.clientX, y: event.clientY, items });
  }

  function close() {
    setState(null);
  }

  const element = state
    ? createPortal(
        <div
          ref={menuRef}
          role="menu"
          className="overflow-menu anim-scale-in"
          style={coords ? { top: coords.top, left: coords.left } : { top: -9999, left: -9999 }}
          onMouseDown={(e) => e.stopPropagation()}
          onClick={(e) => e.stopPropagation()}
          onContextMenu={(e) => e.preventDefault()}
        >
          {state.items.map((item) => (
            <button
              key={item.label}
              type="button"
              role="menuitem"
              disabled={item.disabled}
              className={`overflow-menu-item ${item.danger ? "overflow-menu-item-danger" : ""}`}
              onClick={() => {
                close();
                item.onClick();
              }}
            >
              {item.icon && <Icon name={item.icon} size={14} />}
              {item.label}
            </button>
          ))}
        </div>,
        document.body,
      )
    : null;

  return { open, close, element };
}
