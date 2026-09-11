import { Tooltip } from "./Tooltip";
import "./HelpHint.css";

interface HelpHintProps {
  /** The explanation shown in the tooltip - keep it to one short sentence, long explanations belong in Docs, not here. */
  label: string;
}

/** A small "(?)" next to a label that needs a one-line explanation the
 * first time a user meets it - technical detail stays out of the label
 * itself and lives here instead, one hover away. */
export function HelpHint({ label }: HelpHintProps) {
  return (
    <Tooltip label={label} placement="right">
      <button type="button" className="help-hint" aria-label={label}>
        ?
      </button>
    </Tooltip>
  );
}
