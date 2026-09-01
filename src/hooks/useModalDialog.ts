import { useCallback, useEffect, useRef } from "react";
import type { KeyboardEvent, MouseEvent } from "react";

/**
 * Makes a modal actually behave like one for a keyboard.
 *
 * The app has more than twenty modals and dialogs and, before this, exactly
 * one occurrence of `role="dialog"`, `aria-modal` or `tabIndex` between all
 * of them. In practice that meant: Tab walked straight out of the dialog
 * into the page behind it, focus never moved into the dialog when it
 * opened, focus never returned to whatever opened it when it closed, Escape
 * did nothing, and a screen reader announced a generic group rather than a
 * dialog. Every one of those is a real block for somebody who does not use
 * a mouse - on an app whose destructive actions all live in modals.
 *
 * Returns props to spread onto the backdrop and the panel. The backdrop
 * half keeps the existing click-outside behaviour, which had a subtle fix
 * in it worth preserving (see `useBackdropClose`); it lives here too so a
 * modal has only one thing to wire up.
 *
 * ```tsx
 * const dialog = useModalDialog(onClose, { labelledBy: "delete-title" });
 * <div className="modal-backdrop" {...dialog.backdropProps}>
 *   <div className="modal-panel" {...dialog.panelProps}>
 *     <h2 id="delete-title">…</h2>
 * ```
 */
export function useModalDialog(onClose: () => void, options: { labelledBy?: string; label?: string } = {}) {
  const panelRef = useRef<HTMLDivElement>(null);
  const startedOnBackdrop = useRef(false);
  const previouslyFocused = useRef<HTMLElement | null>(null);

  useEffect(() => {
    previouslyFocused.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;

    // Focus the first thing worth focusing, falling back to the panel
    // itself (which is why it carries `tabIndex={-1}`). Without this, focus
    // stays on whatever was behind the dialog, so the first Tab press moves
    // *within the page behind it* rather than into the dialog.
    const focusable = collectFocusable(panelRef.current);
    (focusable[0] ?? panelRef.current)?.focus();

    const restoreTo = previouslyFocused.current;
    return () => {
      // Restoring focus is what makes a dialog feel like it belongs to the
      // control that opened it: close it and the caret is back on that
      // button, not at the top of the document.
      restoreTo?.focus();
    };
  }, []);

  const onKeyDown = useCallback(
    (event: KeyboardEvent<HTMLDivElement>) => {
      if (event.key === "Escape") {
        event.stopPropagation();
        onClose();
        return;
      }
      if (event.key !== "Tab") return;

      // The trap. Tab from the last focusable wraps to the first and
      // Shift+Tab from the first wraps to the last, so focus cannot leave
      // the dialog while it is open.
      const focusable = collectFocusable(panelRef.current);
      if (focusable.length === 0) {
        event.preventDefault();
        return;
      }
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      const active = document.activeElement;

      if (event.shiftKey && (active === first || active === panelRef.current)) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && active === last) {
        event.preventDefault();
        first.focus();
      }
    },
    [onClose],
  );

  return {
    backdropProps: {
      onMouseDown: (event: MouseEvent<HTMLDivElement>) => {
        // Tracking where the mousedown landed, not just the click: a drag
        // that starts inside the panel and ends on the backdrop synthesizes
        // a click targeting the backdrop, so `target === currentTarget`
        // alone would close the dialog when somebody merely selected text
        // and released outside it.
        startedOnBackdrop.current = event.target === event.currentTarget;
      },
      onClick: (event: MouseEvent<HTMLDivElement>) => {
        if (event.target === event.currentTarget && startedOnBackdrop.current) {
          onClose();
        }
      },
    },
    panelProps: {
      ref: panelRef,
      role: "dialog" as const,
      "aria-modal": true,
      "aria-labelledby": options.labelledBy,
      "aria-label": options.label,
      // Focusable so the panel can hold focus when it contains nothing
      // focusable, and so Escape reaches the handler either way.
      tabIndex: -1,
      onKeyDown,
      // Clicks inside must not reach the backdrop's close handler.
      onClick: (event: MouseEvent<HTMLDivElement>) => event.stopPropagation(),
    },
  };
}

/**
 * Everything inside `container` a keyboard can reach, in document order.
 *
 * Recomputed on every Tab rather than cached: a dialog's focusable set
 * changes while it is open - a button becomes disabled during a save, a
 * wizard reveals the next step - and a stale list would trap focus on an
 * element that is no longer reachable.
 */
function collectFocusable(container: HTMLElement | null): HTMLElement[] {
  if (!container) return [];
  const selector = [
    "a[href]",
    "button:not([disabled])",
    "input:not([disabled])",
    "select:not([disabled])",
    "textarea:not([disabled])",
    '[tabindex]:not([tabindex="-1"])',
  ].join(",");
  return Array.from(container.querySelectorAll<HTMLElement>(selector)).filter(
    // `offsetParent === null` catches a `display: none` ancestor, which is
    // how a hidden wizard step is usually kept mounted.
    (element) => element.offsetParent !== null || element === document.activeElement,
  );
}
