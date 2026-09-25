import { useCallback, useEffect, useRef } from "react";
import type { KeyboardEvent } from "react";

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
 * Returns props to spread onto the backdrop and the panel, so a modal has
 * only one thing to wire up. A click on the backdrop does not close the
 * dialog - see the note on `backdropProps`.
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
      // The panel opts out of Lenis so its own content scrolls; the backdrop
      // must too, or a wheel over the dim area around the panel falls through
      // to Lenis and scrolls the page behind the open dialog. Same opt-out,
      // one level up, so the whole overlay holds the scroll.
      "data-lenis-prevent": true,
      // No click-outside close, deliberately. It threw away whatever was on
      // the panel - a half-filled server form, a connection in progress -
      // for a click that was usually meant for the sidebar behind it, and
      // left no way to tell whether the connection had gone through. The
      // close button and Escape are the ways out; a dialog with work in
      // flight can hand that work to the background instead of dropping it.
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
      // Hands the wheel back to the browser inside the dialog. Lenis captures
      // wheel events on the content wrapper, and these dialogs - unlike the
      // Radix ones - render in the tree rather than in a portal, so anything
      // scrollable inside them scrolled the page underneath instead of
      // itself. Read as the dialog simply refusing to scroll.
      "data-lenis-prevent": true,
      onKeyDown,
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
