/**
 * How long each backend command actually takes, measured at the one place
 * every one of them passes through.
 *
 * "The UI feels slow" is not something to guess at. Almost nothing this app
 * shows is computed locally: a section is a screen's worth of answers from a
 * Node over SSH, and a round trip that takes 900ms looks exactly like a
 * rendering problem from the outside. This tells the two apart, by command
 * name, with no rebuild of the Rust side needed.
 *
 * The numbers live in memory for the session only. Nothing is sent anywhere,
 * and the command's arguments are never touched - only its name, which is a
 * fixed identifier in the Rust command table.
 */

/** Above this, a single call is worth a line in the console on its own. */
const SLOW_COMMAND_MS = 750;

export interface CommandTiming {
  command: string;
  calls: number;
  totalMs: number;
  slowestMs: number;
  /** Round trips that ended in an error - they cost time too. */
  failures: number;
}

const timings = new Map<string, CommandTiming>();

export function recordCommandTiming(command: string, ms: number, failed: boolean): void {
  const existing = timings.get(command) ?? { command, calls: 0, totalMs: 0, slowestMs: 0, failures: 0 };
  existing.calls += 1;
  existing.totalMs += ms;
  existing.slowestMs = Math.max(existing.slowestMs, ms);
  if (failed) existing.failures += 1;
  timings.set(command, existing);

  if (ms >= SLOW_COMMAND_MS) {
    console.warn(`[vibessh] ${command} took ${Math.round(ms)}ms`);
  }
}

/** Everything measured so far, slowest total first - where the time went. */
export function commandTimings(): CommandTiming[] {
  return [...timings.values()].sort((a, b) => b.totalMs - a.totalMs);
}

export function resetCommandTimings(): void {
  timings.clear();
}

/**
 * `vibesshTimings()` in the dev console prints the table.
 *
 * A global rather than a UI panel because this is a diagnostic, and the
 * moment to use it is while something feels slow - which is a moment for
 * opening the console, not for finding a settings page.
 */
declare global {
  interface Window {
    vibesshTimings?: () => void;
    vibesshTimingsReset?: () => void;
  }
}

if (typeof window !== "undefined") {
  window.vibesshTimings = () => {
    const rows = commandTimings().map((timing) => ({
      command: timing.command,
      calls: timing.calls,
      "avg ms": Math.round(timing.totalMs / timing.calls),
      "slowest ms": Math.round(timing.slowestMs),
      "total ms": Math.round(timing.totalMs),
      failures: timing.failures,
    }));
    console.table(rows);
  };
  window.vibesshTimingsReset = resetCommandTimings;
}
