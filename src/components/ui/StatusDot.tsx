import { useTranslation } from "react-i18next";
import type { ServerConnectionStatus } from "@/types/server";
import "./StatusDot.css";

interface StatusDotProps {
  status: ServerConnectionStatus;
  /** Renders the status word next to the dot. Use it wherever there is room:
   * the dot alone is a colour, and a colour alone is not information
   * (WCAG 1.4.1). Off only for the Rail's 40px icon button, which has no
   * room for a word and carries the status in its accessible name instead. */
  withLabel?: boolean;
  className?: string;
}

const STATUS_LABEL_KEY: Record<ServerConnectionStatus, string> = {
  online: "serverStatus.online",
  offline: "serverStatus.offline",
  connecting: "serverStatus.connecting",
  unknown: "serverStatus.unknown",
};

/**
 * A node's connection status, as something other than a hue.
 *
 * Four copies of this used to be written inline as
 * `<span style={{ background: statusColor }} />` - a bare coloured circle
 * with no text, no title and no accessible name. Three problems, in
 * increasing order of severity:
 *
 * 1. `offline` and `unknown` are *the same colour*, so the dot could not
 *    distinguish them even for someone who sees colour perfectly.
 * 2. Nothing announced it, so a screen reader user got a node card that
 *    never mentioned whether the node was reachable.
 * 3. Colour was the only channel, which fails WCAG 1.4.1 for anyone with a
 *    colour vision deficiency - and red/green is the common one.
 *
 * So the dot now differs in *shape* as well as hue (filled, ringed, hollow,
 * hollow-with-a-centre), and the status word is rendered outright wherever
 * the layout has room for it. Where it isn't, the dot itself carries an
 * accessible name; where it is, the dot goes `aria-hidden` so the status is
 * announced exactly once rather than twice.
 */
export function StatusDot({ status, withLabel = false, className }: StatusDotProps) {
  const { t } = useTranslation();
  const label = t(STATUS_LABEL_KEY[status]);

  return (
    <span className={`status-dot-wrap${className ? ` ${className}` : ""}`}>
      <span
        className="status-dot"
        data-status={status}
        // `title` regardless: hovering the dot is how a sighted user checks
        // what a shape means the first time they meet it.
        title={label}
        {...(withLabel ? { "aria-hidden": true } : { role: "img", "aria-label": label })}
      />
      {withLabel && <span className="status-dot-label">{label}</span>}
    </span>
  );
}
