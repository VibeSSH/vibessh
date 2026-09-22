import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { useModalDialog } from "./useModalDialog";
import { OWN_SCROLLERS } from "./useSmoothScroll";

function Modal() {
  const dialog = useModalDialog(() => {}, { label: "test" });
  return (
    <div className="modal-backdrop" {...dialog.backdropProps}>
      <div className="modal-panel" {...dialog.panelProps}>
        <div className="wizard-blueprint-grid" data-testid="scroller">
          <button data-testid="tile">a tile</button>
        </div>
      </div>
    </div>
  );
}

describe("wheel events inside a hand-built dialog", () => {
  it("reach the browser instead of the page's smooth scrolling", () => {
    render(<Modal />);

    // These dialogs render in the tree, inside the element Lenis captures the
    // wheel on - unlike the Radix ones, which portal out of it. Without the
    // opt-out, scrolling a list inside one scrolled the page underneath, which
    // reads as the dialog refusing to scroll at all.
    const target = screen.getByTestId("tile");

    expect(target.closest(OWN_SCROLLERS)).toBe(screen.getByRole("dialog"));
  });

  it("opt the backdrop out too, so the dim area does not scroll the page behind", () => {
    const { container } = render(<Modal />);

    // A wheel over the dim area around the panel targets the backdrop itself.
    // If it did not match a Lenis opt-out, Lenis would scroll the page under
    // the open dialog - the backdrop carries `data-lenis-prevent` for exactly
    // this, so it is its own nearest own-scroller.
    const backdrop = container.querySelector(".modal-backdrop") as HTMLElement;
    expect(backdrop).not.toBeNull();
    expect(backdrop.closest(OWN_SCROLLERS)).toBe(backdrop);
  });
});
