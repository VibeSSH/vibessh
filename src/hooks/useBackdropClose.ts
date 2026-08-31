import { useRef } from "react";
import type { MouseEvent } from "react";

/**
 * A modal backdrop that closes on a plain `onClick={onClose}` also closes
 * when a drag - selecting text, or just an accidental mouse move mid-click
 * - starts inside the modal panel and ends on the backdrop: the browser
 * synthesizes the resulting `click` event targeting the nearest common
 * ancestor of the `mousedown` and `mouseup` elements, which becomes the
 * backdrop itself the moment the drag crosses the panel's edge, even
 * though the interaction never really "clicked the backdrop" as such -
 * `e.target === e.currentTarget` is true on that click regardless, so that
 * check alone doesn't help. Tracking where the `mousedown` itself landed
 * and only closing when *that* also started on the backdrop fixes it:
 * selecting text or dragging out of the panel no longer closes the modal,
 * a real click on the backdrop still does.
 *
 * Usage: spread the result onto the backdrop `div` in place of a plain
 * `onClick` - `<div className="modal-backdrop" {...useBackdropClose(onClose)}>`.
 */
export function useBackdropClose(onClose: () => void) {
  const startedOnBackdrop = useRef(false);

  return {
    onMouseDown: (event: MouseEvent<HTMLDivElement>) => {
      startedOnBackdrop.current = event.target === event.currentTarget;
    },
    onClick: (event: MouseEvent<HTMLDivElement>) => {
      if (event.target === event.currentTarget && startedOnBackdrop.current) {
        onClose();
      }
    },
  };
}
