import { useEffect } from "react";
import type { RefObject } from "react";

/** Calls `onOutside` on a `mousedown` that lands outside `ref`'s element -
 * the same pattern `Rail.tsx`'s notification/account popovers already
 * hand-roll individually, generalized so new popovers (like `RowPicker`)
 * don't repeat it a third time. */
export function useClickOutside(ref: RefObject<HTMLElement | null>, onOutside: () => void, active: boolean) {
  useEffect(() => {
    if (!active) return;
    function handlePointerDown(event: MouseEvent) {
      if (ref.current && !ref.current.contains(event.target as Node)) onOutside();
    }
    document.addEventListener("mousedown", handlePointerDown);
    return () => document.removeEventListener("mousedown", handlePointerDown);
  }, [active, ref, onOutside]);
}
