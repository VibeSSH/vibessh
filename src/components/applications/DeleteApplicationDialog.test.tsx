import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { DeleteApplicationDialog } from "./DeleteApplicationDialog";

/**
 * A confirmation dialog for a destructive, irreversible action. What matters
 * is that it names what is about to be deleted, that both ways out actually
 * work, and that it cannot be double-fired while the deletion is in flight -
 * a second `delete_application` for the same id races the first and produces
 * a confusing "not found".
 */
describe("DeleteApplicationDialog", () => {
  function renderDialog(overrides: Partial<Parameters<typeof DeleteApplicationDialog>[0]> = {}) {
    const onConfirm = vi.fn();
    const onCancel = vi.fn();
    render(
      <DeleteApplicationDialog
        applicationName="Survival"
        busy={false}
        error={null}
        removeFiles={false}
        onRemoveFilesChange={() => {}}
        onConfirm={onConfirm}
        onCancel={onCancel}
        {...overrides}
      />,
    );
    return { onConfirm, onCancel };
  }

  /// The one step here that cannot be undone. It has to be a deliberate
  /// tick, never a default, and it has to reach the caller as typed.
  it("offers removing the files, unticked, and reports the choice", async () => {
    const onRemoveFilesChange = vi.fn();
    renderDialog({ onRemoveFilesChange });

    const checkbox = screen.getByRole("checkbox");
    expect(checkbox).not.toBeChecked();

    await userEvent.click(checkbox);
    expect(onRemoveFilesChange).toHaveBeenCalledWith(true);
  });

  it("names the application being deleted", () => {
    renderDialog();

    // The name is interpolated into a `Trans` sentence and rendered inside a
    // nested <strong>, so the match has to span elements.
    expect(screen.getByRole("heading", { name: "Delete application" })).toBeInTheDocument();
    expect(screen.getByText("Survival")).toBeInTheDocument();
  });

  /**
   * The copy has to describe what delete actually does. It previously said
   * deletion "won't stop a still-running process/unit/container on its own",
   * which stopped being true when delete became a real teardown - and copy
   * that under-describes a destructive action is worse than none.
   */
  it("tells the user what else gets removed", () => {
    renderDialog();

    const body = screen.getByText(/This also removes/).textContent ?? "";
    expect(body).toMatch(/container/);
    expect(body).toMatch(/databases/);
    expect(body).toMatch(/firewall/);
    expect(body).toMatch(/can't be undone/);
    // Files are deliberately kept - saying otherwise would be a lie in the
    // more alarming direction.
    expect(body).toMatch(/working directory are kept/);
  });

  it("confirms when the destructive button is pressed", async () => {
    const { onConfirm, onCancel } = renderDialog();

    await userEvent.click(screen.getByRole("button", { name: "Remove" }));

    expect(onConfirm).toHaveBeenCalledOnce();
    expect(onCancel).not.toHaveBeenCalled();
  });

  it("cancels from the cancel button", async () => {
    const { onConfirm, onCancel } = renderDialog();

    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));

    expect(onCancel).toHaveBeenCalledOnce();
    expect(onConfirm).not.toHaveBeenCalled();
  });

  it("cancels from the close button", async () => {
    const { onCancel } = renderDialog();

    await userEvent.click(screen.getByRole("button", { name: "Close" }));

    expect(onCancel).toHaveBeenCalledOnce();
  });

  /**
   * The guard against a double delete. Without it, an impatient second click
   * fires a second teardown for an id the first one is already removing.
   */
  it("disables both actions while the deletion is in flight", async () => {
    const { onConfirm, onCancel } = renderDialog({ busy: true });

    const remove = screen.getByRole("button", { name: "Remove" });
    const cancel = screen.getByRole("button", { name: "Cancel" });
    expect(remove).toBeDisabled();
    expect(cancel).toBeDisabled();

    await userEvent.click(remove);
    expect(onConfirm).not.toHaveBeenCalled();
    expect(onCancel).not.toHaveBeenCalled();
  });

  /**
   * A failed teardown leaves a running container still holding its port, so
   * the reason has to stay on screen rather than disappearing with the
   * dialog.
   */
  it("shows a failure without closing the dialog", () => {
    renderDialog({ error: "couldn't remove the container: connection reset" });

    expect(screen.getByText(/couldn't remove the container: connection reset/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Remove" })).toBeEnabled();
  });

  it("shows no error region when there is nothing to report", () => {
    renderDialog();

    expect(screen.queryByText(/couldn't/i)).not.toBeInTheDocument();
  });
});
