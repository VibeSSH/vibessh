import "./LivePill.css";

/**
 * "Live" / "out of date" beside a card's title, for a card fed by a file a
 * plugin keeps rewriting - the Minecraft and Restarts tabs. One component so
 * the two say it the same way; the dot is the only colour on it.
 */
export function LivePill({ stale, liveLabel, staleLabel }: { stale: boolean; liveLabel: string; staleLabel: string }) {
  return (
    <span className={`live-pill ${stale ? "live-pill-stale" : ""}`}>
      <span className="live-pill-dot" aria-hidden />
      {stale ? staleLabel : liveLabel}
    </span>
  );
}
