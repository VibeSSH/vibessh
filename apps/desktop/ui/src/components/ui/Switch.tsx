import type { ReactNode } from "react";
import "./Switch.css";

interface SwitchProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label?: ReactNode;
  /**
   * The name a screen reader announces, where the switch is described by
   * text beside it rather than by its own label.
   *
   * Without this the choice was between printing the same sentence twice -
   * once as the row's heading and once beside the switch - or a control
   * that announces nothing at all.
   */
  ariaLabel?: string;
  disabled?: boolean;
  id?: string;
}

/** A real toggle, not a checkbox pretending to be one - `role="switch"` on
 * a native `<button>` gets click/Space/focus handling for free, no `<input
 * type="checkbox">` involved. */
export function Switch({ checked, onChange, label, ariaLabel, disabled, id }: SwitchProps) {
  return (
    <label className={`switch-row ${disabled ? "switch-row-disabled" : ""}`} htmlFor={id}>
      <button
        id={id}
        type="button"
        role="switch"
        aria-checked={checked}
        aria-label={ariaLabel}
        className={`switch-track ${checked ? "switch-track-on" : ""}`}
        disabled={disabled}
        onClick={() => onChange(!checked)}
      >
        <span className="switch-thumb" />
      </button>
      {label && <span className="switch-label">{label}</span>}
    </label>
  );
}
