import { motion, useReducedMotion } from "motion/react";
import "./TabUnderline.css";

interface TabUnderlineProps {
  /**
   * Identifies the strip this underline belongs to. Two strips on screen at
   * once with the same value would animate into each other, so every strip
   * passes its own.
   */
  group: string;
}

/**
 * The line under the selected tab, which slides when the selection moves.
 *
 * **Why this is a component and not CSS.** A `border-bottom` on whichever tab
 * is active cannot animate between two different elements - it can only fade
 * in where it already is. Motion's shared layout does the rest: one element
 * with the same `layoutId` rendered in a different place is understood as the
 * *same* element having moved, and it is animated from where it was to where
 * it now is.
 *
 * Rendered inside the active tab only. The tab strips keep their own markup
 * and their own state; this is the one piece they could not express.
 */
export function TabUnderline({ group }: TabUnderlineProps) {
  // Somebody who asked the system to reduce motion gets the line placed
  // rather than slid. The app's global CSS rule cannot reach this: it caps
  // CSS animation and transition durations, and this movement is neither.
  const still = useReducedMotion();

  return (
    <motion.span
      layoutId={`tab-underline-${group}`}
      className="tab-underline"
      transition={still ? { duration: 0 } : { duration: 0.18, ease: [0.16, 1, 0.3, 1] }}
      aria-hidden="true"
    />
  );
}
