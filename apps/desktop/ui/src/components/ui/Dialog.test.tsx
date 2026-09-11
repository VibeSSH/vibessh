import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Dialog } from "./Dialog";

/**
 * `useModalDialog` already covered the focus trap, Escape and focus restore,
 * and those behaviours are unchanged. What is asserted here is only the three
 * things it did not do - the whole reason this component exists.
 */
describe("Dialog", () => {
  function open(props: Partial<Parameters<typeof Dialog>[0]> = {}) {
    const onClose = vi.fn();
    const result = render(
      <div>
        <button>Behind the dialog</button>
        <Dialog open onClose={onClose} title="Delete application" {...props}>
          <div className="modal-body">
            <button>Confirm</button>
          </div>
        </Dialog>
      </div>,
    );
    return { onClose, ...result };
  }

  it("renders the panel outside the tree it was written in", () => {
    // `position: fixed` stops meaning "relative to the window" under any
    // ancestor with a transform or filter, so the panel has to leave the
    // subtree that a card animation might one day be added to.
    const { container } = open();

    expect(container.querySelector('[role="dialog"]')).toBeNull();
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });

  it("stops the page behind it from scrolling", () => {
    const { unmount } = open();

    expect(document.body).toHaveStyle({ overflow: "hidden" });

    unmount();
    expect(document.body).not.toHaveStyle({ overflow: "hidden" });
  });

  it("hides the page behind it from a screen reader", () => {
    // The dimmed backdrop says "this is unreachable" to someone who can see
    // it. Without this, a screen reader still walks the page underneath.
    const { container } = open();

    // Still in the document, but no longer in the accessibility tree - which
    // is exactly what `getByRole` refuses to return and `textContent` still
    // sees.
    expect(container.textContent).toContain("Behind the dialog");
    expect(screen.queryByRole("button", { name: "Behind the dialog" })).toBeNull();
    expect(screen.getByRole("button", { name: "Confirm" })).toBeInTheDocument();
  });

  it("refuses to close mid-action when it is not dismissable", async () => {
    const { onClose } = open({ dismissable: false });

    await userEvent.keyboard("{Escape}");

    expect(onClose).not.toHaveBeenCalled();
  });

  it("closes on Escape the rest of the time", async () => {
    const { onClose } = open();

    await userEvent.keyboard("{Escape}");

    expect(onClose).toHaveBeenCalledOnce();
  });
});
