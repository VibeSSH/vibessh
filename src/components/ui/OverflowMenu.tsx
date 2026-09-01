import { useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Icon } from "./Icon";
import { IconButton } from "./IconButton";
import { useClickOutside } from "@/hooks/useClickOutside";
import "./OverflowMenu.css";

export interface OverflowMenuItem {
  label: string;
  icon?: string;
  onClick: () => void;
  danger?: boolean;
  disabled?: boolean;
}

interface OverflowMenuProps {
  items: OverflowMenuItem[];
  ariaLabel: string;
}

const VIEWPORT_MARGIN = 8;
const MENU_GAP = 4;

/** A small "..." menu for actions that shouldn't be the loudest thing on a
 * card/row - destructive or rarely-used actions (leave network, delete,
 * rename) live here instead of as a standalone button competing with the
 * card's real primary action. */
export function OverflowMenu({ items, ariaLabel }: OverflowMenuProps) {
  const [open, setOpen] = useState(false);
  const [coords, setCoords] = useState<{ top: number; left: number } | null>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);

  useClickOutside(menuRef, () => setOpen(false), open);

  useLayoutEffect(() => {
    if (!open || !triggerRef.current) return;
    function reposition() {
      const anchor = triggerRef.current!.getBoundingClientRect();
      const menuRect = menuRef.current?.getBoundingClientRect() ?? { width: 0, height: 0 };
      let left = anchor.right - menuRect.width;
      left = Math.min(Math.max(left, VIEWPORT_MARGIN), window.innerWidth - menuRect.width - VIEWPORT_MARGIN);
      let top = anchor.bottom + MENU_GAP;
      if (top + menuRect.height > window.innerHeight - VIEWPORT_MARGIN) {
        top = Math.max(VIEWPORT_MARGIN, anchor.top - menuRect.height - MENU_GAP);
      }
      setCoords({ top, left });
    }
    reposition();
    window.addEventListener("resize", reposition);
    window.addEventListener("scroll", reposition, true);
    return () => {
      window.removeEventListener("resize", reposition);
      window.removeEventListener("scroll", reposition, true);
    };
  }, [open]);

  return (
    <>
      <IconButton ref={triggerRef} icon="more-vertical" size="sm" title={ariaLabel} onClick={() => setOpen((o) => !o)} />
      {open &&
        createPortal(
          <div
            ref={menuRef}
            role="menu"
            className="overflow-menu anim-scale-in"
            style={coords ? { top: coords.top, left: coords.left } : { top: -9999, left: -9999 }}
            onMouseDown={(e) => e.stopPropagation()}
            onClick={(e) => e.stopPropagation()}
          >
            {items.map((item) => (
              <button
                key={item.label}
                type="button"
                role="menuitem"
                disabled={item.disabled}
                className={`overflow-menu-item ${item.danger ? "overflow-menu-item-danger" : ""}`}
                onClick={() => {
                  setOpen(false);
                  item.onClick();
                }}
              >
                {item.icon && <Icon name={item.icon} size={14} />}
                {item.label}
              </button>
            ))}
          </div>,
          document.body,
        )}
    </>
  );
}
