import { useState } from "react";
import { useTranslation } from "react-i18next";
import { IconButton } from "./IconButton";
import "./HostAddress.css";

/** Fixed-width, not length-matched to the real value - a mask that grows
 * with the string it hides still leaks its shape (an IPv4 reads differently
 * than a hostname at a glance), and a fixed placeholder also never causes
 * layout jank as reveal is toggled. */
const MASK = "••••••••";

interface HostAddressProps {
  /** The sensitive part - a bare host, or "host:port". Never put this in a
   * native `title=` on a wrapping element; this component only ever emits
   * a `title` for the real value once `revealed` is true. */
  value: string;
  /** Always-visible text shown before the masked/revealed value, e.g. "root@" - a username alone doesn't identify or locate the server the way its address does. */
  prefix?: string;
  className?: string;
  /** Set false where an eye toggle can't be nested (e.g. inside another
   * `<button>`, like ServerCard's terminal-preview corner) - renders
   * permanently masked with no control of its own. The real address is
   * expected to be revealable from this same card's primary address line
   * instead. */
  interactive?: boolean;
}

/**
 * The one place a server's host/IP gets rendered anywhere in the app -
 * masked by default, revealed only after an explicit click on the eye icon
 * (per-mount state, so navigating away or re-rendering the list re-masks
 * it). Originally a one-off in Rail's instance tooltip; pulled out here so
 * every other place a raw IP was showing in plain text (Dashboard node
 * cards, ServerCard, Files/Terminal/Monitor/Actions page headers, the
 * Servers list, Teams' server list) gets the same treatment instead of each
 * screen inventing its own masking.
 */
export function HostAddress({ value, prefix, className, interactive = true }: HostAddressProps) {
  const { t } = useTranslation();
  const [revealed, setRevealed] = useState(false);

  return (
    <span className={`host-address ${className ?? ""}`}>
      {prefix && <span className="host-address-prefix">{prefix}</span>}
      <span className="host-address-value" title={revealed ? value : undefined}>
        {revealed ? value : MASK}
      </span>
      {interactive && (
        <IconButton
          icon={revealed ? "eye-off" : "eye"}
          size="sm"
          className="host-address-eye"
          title={revealed ? t("rail.hideHostAria") : t("rail.revealHostAria")}
          onClick={(e) => {
            e.stopPropagation();
            setRevealed((r) => !r);
          }}
        />
      )}
    </span>
  );
}
