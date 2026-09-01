import type { ReactNode } from "react";
import "./Switch.css";

interface SwitchProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label?: ReactNode;
  disabled?: boolean;
  id?: string;
}

/** A real toggle, not a checkbox pretending to be one - `role="switch"` on
 * a native `<button>` gets click/Space/focus handling for free, no `<input
 * type="checkbox">` involved. */
export function Switch({ checked, onChange, label, disabled, id }: SwitchProps) {
  return (
    <label className={`switch-row ${disabled ? "switch-row-disabled" : ""}`} htmlFor={id}>
      <button
        id={id}
        type="button"
        role="switch"
        aria-checked={checked}
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
