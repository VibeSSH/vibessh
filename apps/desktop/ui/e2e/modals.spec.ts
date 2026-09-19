import { test, expect } from "@playwright/test";

/**
 * A tall modal at the smallest supported window must stay inside the viewport
 * and scroll its own content (so its footer buttons stay reachable), and the
 * backdrop must hold the scroll so the page behind cannot be wheeled. Only
 * meaningful at 960x600, so the other viewport projects skip it.
 */
test.describe("modals at the minimum window", () => {
  test.skip(({ viewport }) => !viewport || viewport.width !== 960, "min-size (960x600) only");

  test("create-application wizard fits, scrolls, and locks the background", async ({ page }) => {
    await page.goto("/?fixtures=1#/applications");
    await expect(page.locator(".sidebar")).toBeVisible();

    // Non-accented substrings so this matches whether the app renders in
    // Polish ("Utworz aplikacje") or English ("Create application").
    await page.getByRole("button", { name: /aplikacj|application/i }).click();

    const panel = page.locator(".modal-panel").first();
    await expect(panel).toBeVisible();

    const info = await panel.evaluate((el) => {
      const r = el.getBoundingClientRect();
      return {
        top: r.top,
        bottom: r.bottom,
        vh: window.innerHeight,
        overflowY: getComputedStyle(el).overflowY,
        scrollable: el.scrollHeight > el.clientHeight,
      };
    });

    // Panel stays within the window top and bottom.
    expect(info.top).toBeGreaterThanOrEqual(0);
    expect(info.bottom).toBeLessThanOrEqual(info.vh + 1);
    // Its own content scrolls rather than overflowing the window.
    expect(info.overflowY).toBe("auto");

    // No horizontal page scroll while the modal is open.
    const overflowX = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
    expect(overflowX).toBeLessThanOrEqual(1);

    // The backdrop opts out of the page's smooth scrolling, so wheeling the dim
    // area cannot scroll the page behind the dialog.
    const backdropPrevents = await page.locator(".modal-backdrop").first().evaluate((el) => el.hasAttribute("data-lenis-prevent"));
    expect(backdropPrevents).toBe(true);

    // Escape closes it and leaves nothing behind.
    await page.keyboard.press("Escape");
    await expect(page.locator(".modal-backdrop")).toHaveCount(0);
  });
});
