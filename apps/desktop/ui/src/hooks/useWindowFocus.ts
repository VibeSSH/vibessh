import { useEffect, useRef } from "react";

/**
 * Runs something when the window is looked at again.
 *
 * The app shows files it does not own. Anything can change them while it is
 * in the background - an editor on the machine the files live on, a deploy, a
 * process writing a log - and until the window comes back there is nobody to
 * show the change to. So the moment of return is when it is worth asking.
 *
 * Both events are needed and neither is enough. `focus` covers alt-tabbing
 * between windows; `visibilitychange` covers a window that was never
 * unfocused but was hidden - minimised, or behind a full-screen window - and
 * on some systems that happens without a blur.
 *
 * The callback is held in a ref so a listener is not torn down and rebuilt on
 * every render, which would drop the very event it is there to catch.
 */
export function useWindowFocus(onFocus: () => void, enabled = true) {
  const handler = useRef(onFocus);
  handler.current = onFocus;

  useEffect(() => {
    if (!enabled) return;

    const run = () => handler.current();
    const onVisible = () => {
      if (document.visibilityState === "visible") run();
    };

    window.addEventListener("focus", run);
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      window.removeEventListener("focus", run);
      document.removeEventListener("visibilitychange", onVisible);
    };
  }, [enabled]);
}
