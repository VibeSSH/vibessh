import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { Markdown } from "./Markdown";

/**
 * The renderer serves two callers with different markdown: the guide, whose
 * files live in this repository, and Vibe AI, whose answers are written by a
 * model. These cover the shapes only the second one produces.
 */
describe("Markdown", () => {
  it("keeps a numbered list numbered across its sub-points", () => {
    // The exact shape the assistant answers in - a step, its sub-points
    // indented under it, then the next step.
    const { container } = render(
      <Markdown
        source={[
          "1. Otwórz aplikację.",
          "2. Dodaj zmienne środowiskowe:",
          "   - `REDIS_HOST`: adres hosta.",
          "   - `REDIS_PORT`: domyślnie 6379.",
          "3. Zapisz.",
        ].join("\n")}
      />,
    );

    // One list, not three: a flat reader ends the list at the first indented
    // bullet and restarts the numbering at 1 on the step after it.
    const lists = container.querySelectorAll("ol");
    expect(lists).toHaveLength(1);
    expect(lists[0].children).toHaveLength(3);

    // The sub-points hang off the step they belong to, not off the list.
    const secondStep = lists[0].children[1];
    expect(secondStep.querySelectorAll("ul li")).toHaveLength(2);
  });

  it("keeps one numbered list when the model leaves a blank line between steps", () => {
    // A "loose" list. The model writes them this way, and each item ending
    // its own list is what made three steps render as "1. 1. 1.".
    const { container } = render(
      <Markdown
        source={[
          "1. **Problem**: proxy nie startuje.",
          "",
          "2. **Przyczyna**: port jest zajęty.",
          "",
          "3. **Co zrobić**: zmień port.",
        ].join("\n")}
      />,
    );

    const lists = container.querySelectorAll("ol");
    expect(lists).toHaveLength(1);
    expect(lists[0].children).toHaveLength(3);
  });

  it("still ends the list when a paragraph follows the blank line", () => {
    const { container } = render(<Markdown source={["- pierwszy", "- drugi", "", "To już akapit."].join("\n")} />);

    expect(container.querySelectorAll("ul li")).toHaveLength(2);
    expect(container.querySelector("p")?.textContent).toBe("To już akapit.");
  });

  it("renders the markers as elements rather than as their own characters", () => {
    render(<Markdown source="**Problem:** brak `REDIS_HOST`." />);

    expect(screen.getByText("Problem:").tagName).toBe("STRONG");
    expect(screen.getByText("REDIS_HOST").tagName).toBe("CODE");
    expect(screen.queryByText(/\*\*/)).toBeNull();
  });

  it("hands a link to the caller instead of following it", async () => {
    const onLinkClick = vi.fn();
    render(<Markdown source="Zobacz [dokumentację](https://example.test/docs)." onLinkClick={onLinkClick} />);

    await userEvent.click(screen.getByText("dokumentację"));

    expect(onLinkClick).toHaveBeenCalledWith("https://example.test/docs");
  });

  it("does not load an image when the caller has not resolved one", () => {
    // Without a resolver the only source of URLs is whoever wrote the
    // markdown, and a model's answer is not one this app fetches from.
    const { container } = render(<Markdown source="![wykres](https://example.test/a.png)" />);

    expect(container.querySelector("img")).toBeNull();
    expect(screen.getByText("wykres")).toBeTruthy();
  });

  it("still resolves images for the caller that supplies them", () => {
    const { container } = render(<Markdown source="![ekran](ports.png)" resolveImage={() => "/assets/ports.png"} />);

    expect(container.querySelector("img")?.getAttribute("src")).toBe("/assets/ports.png");
  });
});
