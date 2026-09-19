import { test, expect, type Page } from "@playwright/test";

/**
 * The views that make up the app, keyed to a hash route. The server-scoped
 * tools take a fixture server id (`vps`). Detail pages that need an id created
 * at runtime (application/team detail) are covered by their parent list here
 * and by the interactive checks in the audit, not by a hard-coded id.
 */
const ROUTES: Array<[name: string, hash: string]> = [
  ["dashboard", "/"],
  ["servers", "/servers"],
  ["applications", "/applications"],
  ["vibe-network", "/vibe-network"],
  ["database-hosts", "/database-hosts"],
  ["vibe-ai", "/vibe-ai"],
  ["teams", "/teams"],
  ["monitor", "/monitor/vps"],
  ["actions", "/actions/vps"],
  ["firewall", "/firewall/vps"],
  ["port-forwarding", "/port-forwarding/vps"],
  ["files", "/files/vps"],
  ["terminal", "/terminal/vps"],
  ["pterodactyl", "/pterodactyl"],
  ["settings", "/settings"],
  ["guide", "/guide"],
];

async function open(page: Page, hash: string) {
  await page.goto(`/?fixtures=1#${hash}`);
  // The sidebar is part of the app shell; if a view crashed (no error boundary)
  // the whole tree unmounts, so this both waits for render and catches crashes.
  await expect(page.locator(".sidebar")).toBeVisible();
  // Let metric polls / async lists settle so late content is measured too.
  await page.waitForTimeout(350);
}

test.describe("layout", () => {
  for (const [name, hash] of ROUTES) {
    test(`no horizontal page scroll: ${name}`, async ({ page }) => {
      await open(page, hash);

      // The whole document must not scroll sideways. Intentionally-scrollable
      // inner containers (tables, terminals, code) have their own overflow and
      // do not push the document width, so this stays specific to real bugs.
      const overflow = await page.evaluate(() => {
        const de = document.documentElement;
        return de.scrollWidth - de.clientWidth;
      });
      expect(overflow, `${name} overflows the viewport by ${overflow}px`).toBeLessThanOrEqual(1);

      // And the view actually rendered its own content, not just the shell.
      const mainHasContent = await page.evaluate(() => {
        const main = document.querySelector(".app-content-inner");
        return !!main && main.textContent!.trim().length > 0;
      });
      expect(mainHasContent, `${name} rendered no content`).toBe(true);
    });
  }
});
