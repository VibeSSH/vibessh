import { useEffect, useRef } from "react";

/**
 * Every polling interval in the app, in one place.
 *
 * They were previously five separate `POLL_INTERVAL_MS` constants in five
 * files, which made the aggregate load invisible: nobody reading any one of
 * them could see that a dashboard with ten Nodes was issuing roughly a
 * hundred SSH round trips a minute, forever. Collected here so the total is
 * something you can look at.
 *
 * Each of these opens an SSH channel per Node per tick, so the numbers are a
 * real cost, not a rendering preference.
 */
export const POLL_INTERVALS = {
  /** Application console output - the only one a user watches live. */
  console: 2000,
  /** One Application's status and resource usage on its detail page. */
  applicationDetail: 5000,
  /** The Monitor page's process/metrics table, while it is open. */
  monitor: 5000,
  /** CPU/RAM/uptime behind each Dashboard node card. */
  serverMetrics: 6000,
  /** Reachability dot on each server card. */
  serverPing: 15000,
  /** The Dashboard's own aggregate refresh. */
  dashboardOverview: 20000,
  /** Due-backup check. Not a network poll - see `pauseWhenHidden`. */
  backupScheduler: 15 * 60 * 1000,
} as const;

interface PollingOptions {
  /** Skip polling entirely - e.g. nothing selected, or no servers yet. */
  enabled?: boolean;
  /**
   * Stop while the window is hidden, and poll once immediately when it
   * comes back. Default `true`.
   *
   * This is the single biggest saving available here. Without it every
   * interval keeps firing while the app is minimised or on another
   * workspace, so a machine left running overnight spends the whole night
   * opening SSH channels to every Node for readings nobody can see.
   *
   * Pass `false` only for work that must happen whether or not anyone is
   * looking - the backup scheduler is the one case.
   */
  pauseWhenHidden?: boolean;
}

/**
 * Runs `callback` on an interval, pausing while the window is hidden.
 *
 * **Never overlaps itself.** The previous per-file `setInterval` loops fired
 * on a fixed schedule regardless of whether the last poll had finished, so a
 * Node slow enough to take longer than its interval accumulated concurrent
 * in-flight polls - each holding an SSH channel - until it recovered. Here a
 * tick that arrives while the previous one is still running is skipped.
 *
 * `callback` is held in a ref rather than being an effect dependency, so a
 * caller does not have to memoise it to avoid tearing down and rebuilding
 * the interval on every render.
 */
export function usePolling(callback: () => void | Promise<void>, intervalMs: number, options: PollingOptions = {}): void {
  const { enabled = true, pauseWhenHidden = true } = options;
  const callbackRef = useRef(callback);
  callbackRef.current = callback;

  useEffect(() => {
    if (!enabled) return;

    let intervalId: number | undefined;
    let running = false;
    let disposed = false;

    async function tick() {
      // A tick arriving while the previous one is still in flight is
      // dropped, not queued: the next one is a few seconds away and will
      // read fresher data anyway.
      if (running || disposed) return;
      running = true;
      try {
        await callbackRef.current();
      } finally {
        running = false;
      }
    }

    function start() {
      if (intervalId !== undefined || disposed) return;
      void tick();
      intervalId = window.setInterval(() => void tick(), intervalMs);
    }

    function stop() {
      if (intervalId === undefined) return;
      window.clearInterval(intervalId);
      intervalId = undefined;
    }

    function onVisibilityChange() {
      if (document.hidden) {
        stop();
      } else {
        // Restarting polls immediately, so coming back to the window shows
        // current data rather than whatever was on screen when it was
        // hidden plus a full interval's wait.
        start();
      }
    }

    if (pauseWhenHidden && document.hidden) {
      // Nothing to show yet, so nothing to fetch - the visibility listener
      // below starts it when the window comes back.
    } else {
      start();
    }

    if (pauseWhenHidden) {
      document.addEventListener("visibilitychange", onVisibilityChange);
    }

    return () => {
      disposed = true;
      stop();
      if (pauseWhenHidden) {
        document.removeEventListener("visibilitychange", onVisibilityChange);
      }
    };
  }, [enabled, intervalMs, pauseWhenHidden]);
}
