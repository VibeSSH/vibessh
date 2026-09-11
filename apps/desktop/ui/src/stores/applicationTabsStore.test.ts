import { beforeEach, describe, expect, it, vi } from "vitest";
import { useApplicationTabsStore } from "./applicationTabsStore";

const STORED = "vibessh.applicationTabs";

function reset() {
  window.localStorage.clear();
  useApplicationTabsStore.setState({ tabs: [] });
}

/**
 * Which tab inside an Application was last open.
 *
 * Every hop between two Applications landed on Overview, so copying a value
 * from one into the other meant walking the same three clicks back on every
 * trip. The strip's whole point is that going back and forth is one click;
 * these pin that the click arrives where it was left.
 */
describe("applicationTabsStore: the remembered tab", () => {
  beforeEach(reset);

  it("remembers where an application was left, and writes it down", () => {
    const store = useApplicationTabsStore.getState();
    store.open({ id: "app-1", name: "proxy" });

    useApplicationTabsStore.getState().rememberTab("app-1", "logs");

    expect(useApplicationTabsStore.getState().tabs[0].lastTab).toBe("logs");
    expect(JSON.parse(window.localStorage.getItem(STORED) ?? "[]")[0].lastTab).toBe("logs");
  });

  /** `open` runs on every visit, carrying only an id and a name - so this is
   *  the path that would silently forget on the trip that matters. */
  it("keeps it through a visit that renames the application", () => {
    const store = useApplicationTabsStore.getState();
    store.open({ id: "app-1", name: "proxy" });
    useApplicationTabsStore.getState().rememberTab("app-1", "ports");

    useApplicationTabsStore.getState().open({ id: "app-1", name: "proxy-renamed" });

    const tab = useApplicationTabsStore.getState().tabs[0];
    expect(tab.name).toBe("proxy-renamed");
    expect(tab.lastTab).toBe("ports");
  });

  /** The page writes this on every render that changes the tab, so an
   *  unchanged value must not produce a new array - that is a re-render for
   *  everything subscribed to the strip, on a loop. */
  it("returns the very same state when nothing changed", () => {
    useApplicationTabsStore.getState().open({ id: "app-1", name: "proxy" });
    useApplicationTabsStore.getState().rememberTab("app-1", "logs");
    const before = useApplicationTabsStore.getState().tabs;

    useApplicationTabsStore.getState().rememberTab("app-1", "logs");

    expect(useApplicationTabsStore.getState().tabs).toBe(before);
  });

  it("ignores an application that is not in the strip", () => {
    useApplicationTabsStore.getState().rememberTab("never-opened", "logs");

    expect(useApplicationTabsStore.getState().tabs).toEqual([]);
  });

  /** The loader runs once at import, so a reload is reached by importing the
   *  module again - which is also the only way to exercise its defensiveness
   *  against a stored value an older or newer version wrote. */
  it("reads a stored tab back after a reload, and refuses one that is not a string", async () => {
    window.localStorage.setItem(STORED, JSON.stringify([
      { id: "app-1", name: "proxy", lastTab: "databases" },
      { id: "app-2", name: "lobby" },
      { id: "app-3", name: "limbo", lastTab: 7 },
    ]));
    vi.resetModules();

    const fresh = await import("./applicationTabsStore");
    const tabs = fresh.useApplicationTabsStore.getState().tabs;

    expect(tabs.map((tab) => tab.lastTab)).toEqual(["databases", undefined, undefined]);
  });
});
