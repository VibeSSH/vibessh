import type { ReactNode } from "react";
import { Icon } from "./Icon";
import "./Checkbox.css";

interface CheckboxProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: ReactNode;
  disabled?: boolean;
}

/**
 * The app's one styled checkbox - every checkbox before this (the Minecraft
 * EULA field, role permission toggles, the "back up before save" file
 * editor preference) was a bare, unstyled native `<input type="checkbox">`,
 * which is exactly the "looks unfinished" gap Badge/Switch/Tooltip already
 * closed for their own controls. The native input stays in the DOM
 * (visually hidden, not `display:none`) so keyboard focus/activation and
 * screen readers keep working the normal way - only its paint is replaced.
 */
export function Checkbox({ checked, onChange, label, disabled }: CheckboxProps) {
  return (
    <label className={`checkbox ${disabled ? "checkbox-disabled" : ""}`}>
      <input type="checkbox" className="checkbox-input" checked={checked} disabled={disabled} onChange={(e) => onChange(e.target.checked)} />
      <span className="checkbox-box">
        <Icon name="check" size={12} className="checkbox-check" />
      </span>
      <span className="checkbox-label">{label}</span>
    </label>
  );
}
