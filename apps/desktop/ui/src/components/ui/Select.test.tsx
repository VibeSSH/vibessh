import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { Select } from "./Select";

/**
 * The dropdown replaced a native `<select>` at every call site, so the
 * behaviour worth pinning down is the part where the two disagree.
 *
 * Radix reads an empty value as "nothing chosen yet" and draws the
 * placeholder instead of a label. Several forms here list the empty choice as
 * a real option - "No role", "Do not move the databases", "Nothing selected"
 * - and those must read as chosen, because they are.
 */
describe("Select", () => {
  const items = [
    { value: "", label: "Nothing selected" },
    { value: "node:1", label: "Skytop" },
  ];

  it("names the empty option on the trigger instead of leaving it blank", () => {
    render(<Select value="" onChange={() => {}} items={items} aria-label="Subject" />);

    expect(screen.getByRole("combobox", { name: "Subject" })).toHaveTextContent("Nothing selected");
  });

  it("reports the empty option as an empty value, not as its internal stand-in", () => {
    const onChange = vi.fn();
    // The list is portalled and only exists while open, and jsdom has no
    // layout for the popper to open into. The choice is made instead through
    // the hidden native select Radix keeps for form submission, which is why
    // this renders inside a form and gives the control a name.
    const { container } = render(
      <form>
        <Select value="node:1" onChange={onChange} items={items} name="subject" aria-label="Subject" />
      </form>,
    );
    const native = container.querySelector("select");
    expect(native).not.toBeNull();

    const empty = Array.from(native!.options).find((option) => option.textContent === "Nothing selected");
    fireEvent.change(native!, { target: { value: empty!.value } });

    expect(onChange).toHaveBeenCalledWith("");
  });

  it("falls back to the placeholder when the empty value is not an option", () => {
    render(<Select value="" onChange={() => {}} items={[{ value: "tcp", label: "TCP" }]} placeholder="Choose a port" aria-label="Port" />);

    expect(screen.getByRole("combobox", { name: "Port" })).toHaveTextContent("Choose a port");
  });
});
