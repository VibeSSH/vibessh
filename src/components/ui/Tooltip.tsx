import { cloneElement, isValidElement, useLayoutEffect, useRef, useState } from "react";
import type { FocusEvent, MouseEvent, ReactElement, Ref } from "react";
import { createPortal } from "react-dom";
import "./Tooltip.css";

type Placement = "top" | "bottom" | "left" | "right";

interface TooltipProps {
  label: string;
  children: ReactElement;
  placement?: Placement;
  /** ms before the tooltip appears on hover/focus - default sits inside the 300-500ms window this app's tooltips are specced to use. */
  delay?: number;
}

const GAP = 8;
const VIEWPORT_MARGIN = 8;

/** Reads a ReactElement's own `ref` without assuming a particular React
 * version's prop shape - works whether `ref` lives as a top-level element
 * field (18) or has been folded into props (19). */
function readRef(element: ReactElement): Ref<HTMLElement> | undefined {
  return (element as unknown as { ref?: Ref<HTMLElement> }).ref ?? (element.props as { ref?: Ref<HTMLElement> })?.ref;
}

/**
 * Shared floating tooltip - every icon-only button in the app needs one
 * (see `IconButton`, which wraps every button in this by default), so this
 * lives once here rather than each surface hand-rolling its own hover
 * panel the way `Rail.tsx`'s `RailInstanceButton` already does for its own,
 * richer, non-reusable status popover.
 *
 * Clones the single child element - attaches a merged ref (to measure its
 * position) plus hover/focus handlers - so it wraps any host element with
 * no DOM change and no required call-site restructuring.
 */
export function Tooltip({ label, children, placement = "top", delay = 400 }: TooltipProps) {
  const [visible, setVisible] = useState(false);
  const [coords, setCoords] = useState<{ top: number; left: number } | null>(null);
  const anchorRef = useRef<HTMLElement | null>(null);
  const tooltipRef = useRef<HTMLDivElement | null>(null);
  const timeoutRef = useRef<number | undefined>(undefined);

  function show() {
    window.clearTimeout(timeoutRef.current);
    timeoutRef.current = window.setTimeout(() => setVisible(true), delay);
  }

  function hide() {
    window.clearTimeout(timeoutRef.current);
    setVisible(false);
  }

  useLayoutEffect(() => {
    if (!visible || !anchorRef.current) return;
    const anchor = anchorRef.current.getBoundingClientRect();
    const rect = tooltipRef.current?.getBoundingClientRect() ?? { width: 0, height: 0 };

    let top = 0;
    let left = 0;
    if (placement === "top") {
      top = anchor.top - rect.height - GAP;
      left = anchor.left + anchor.width / 2 - rect.width / 2;
    } else if (placement === "bottom") {
      top = anchor.bottom + GAP;
      left = anchor.left + anchor.width / 2 - rect.width / 2;
    } else if (placement === "left") {
      top = anchor.top + anchor.height / 2 - rect.height / 2;
      left = anchor.left - rect.width - GAP;
    } else {
      top = anchor.top + anchor.height / 2 - rect.height / 2;
      left = anchor.right + GAP;
    }

    // Clamp inside the viewport rather than flipping sides - good enough
    // for short one-line labels, and keeps this from turning into a full
    // floating-ui-style collision-detection port.
    left = Math.min(Math.max(left, VIEWPORT_MARGIN), window.innerWidth - rect.width - VIEWPORT_MARGIN);
    top = Math.min(Math.max(top, VIEWPORT_MARGIN), window.innerHeight - rect.height - VIEWPORT_MARGIN);
    setCoords({ top, left });
  }, [visible, placement]);

  useLayoutEffect(() => () => window.clearTimeout(timeoutRef.current), []);

  if (!label || !isValidElement(children)) return children;

  const childRef = readRef(children);
  const child = children as ReactElement<Record<string, unknown>>;

  return (
    <>
      {cloneElement(child, {
        ref: (node: HTMLElement | null) => {
          anchorRef.current = node;
          if (typeof childRef === "function") childRef(node);
          else if (childRef && typeof childRef === "object") (childRef as { current: HTMLElement | null }).current = node;
        },
        onMouseEnter: (e: MouseEvent) => {
          show();
          (child.props.onMouseEnter as ((e: MouseEvent) => void) | undefined)?.(e);
        },
        onMouseLeave: (e: MouseEvent) => {
          hide();
          (child.props.onMouseLeave as ((e: MouseEvent) => void) | undefined)?.(e);
        },
        onFocus: (e: FocusEvent) => {
          show();
          (child.props.onFocus as ((e: FocusEvent) => void) | undefined)?.(e);
        },
        onBlur: (e: FocusEvent) => {
          hide();
          (child.props.onBlur as ((e: FocusEvent) => void) | undefined)?.(e);
        },
      })}
      {visible &&
        createPortal(
          <div
            ref={tooltipRef}
            role="tooltip"
            className="tooltip anim-scale-in"
            style={coords ? { top: coords.top, left: coords.left } : { top: -9999, left: -9999 }}
          >
            {label}
          </div>,
          document.body,
        )}
    </>
  );
}
