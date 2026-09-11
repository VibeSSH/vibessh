import { PointerSensor } from "@dnd-kit/core";
import { useSortable } from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import type { ReactNode } from "react";
import "./SortableApplicationCard.css";

/**
 * A pointer sensor that ignores presses that began on something clickable.
 *
 * An Application card is mostly buttons - start, restart, stop, delete, open.
 * Without this, holding one of them for a moment would pick the whole card up
 * instead of pressing it, which is a surprising way to lose a click on a
 * button that stops a server.
 */
export class CardPointerSensor extends PointerSensor {
  static activators = [
    {
      eventName: "onPointerDown" as const,
      handler: ({ nativeEvent }: { nativeEvent: PointerEvent }) => {
        const target = nativeEvent.target;
        if (!(target instanceof HTMLElement)) return true;
        return target.closest("button, a, input, select, textarea") === null;
      },
    },
  ];
}

/**
 * One draggable card.
 *
 * Wraps rather than modifies `ApplicationCard`: the card is used on screens
 * that have no ordering to speak of, and it already owns a click - selecting
 * itself - that a drag must not swallow. Press-and-hold is what separates the
 * two, and it lives in the sensor's activation constraint rather than in any
 * code here.
 */
export function SortableApplicationCard({ id, children }: { id: string; children: ReactNode }) {
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({ id });

  // `useSortable` also hands back `role="button"` and a `tabIndex` for its
  // keyboard sensor. Both are dropped here: the card inside is already a
  // `role="button"` with `aria-pressed` for selecting itself, so spreading
  // them would nest one button inside another and put a second tab stop on
  // every card - advertising a keyboard drag that is not wired up anyway.
  const { role: _role, tabIndex: _tabIndex, ...dragAttributes } = attributes;

  return (
    <div
      ref={setNodeRef}
      className={`sortable-card ${isDragging ? "sortable-card-dragging" : ""}`.trim()}
      style={{
        // `CSS.Transform` rather than a hand-written string: it emits a
        // translate-only transform, which keeps the card from being scaled by
        // the layout animation and blurring its text mid-drag.
        transform: CSS.Transform.toString(transform),
        transition,
      }}
      {...dragAttributes}
      {...listeners}
    >
      {children}
    </div>
  );
}
