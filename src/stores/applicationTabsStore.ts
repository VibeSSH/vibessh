import { create } from "zustand";

/**
 * The Applications you have open, as tabs.
 *
 * Managing a set of servers means going back and forth between them
 * constantly - checking a proxy's console against a backend's logs, comparing
 * two ports - and every one of those trips went through the Applications
 * list. The tabs turn it into one click.
 *
 * Kept in `localStorage` rather than in the database: which Applications
 * somebody happens to have open is a property of this desktop's current
 * session, not of the infrastructure, and it must survive a reload without
 * being worth a migration.
 */
export interface ApplicationTab {
  id: string;
  name: string;
  /**
   * Which tab inside the Application was last looked at - "logs", "ports",
   * and so on.
   *
   * Switching between two Applications used to land on Overview every time,
   * so comparing a port against a port, or copying a database's details into
   * another Application's environment, meant re-walking the same three clicks
   * on every trip. The strip exists to make going back and forth one click;
   * this is what makes that click arrive where you left off.
   *
   * A plain `string`: which tabs exist is `ApplicationDetail`'s business, and
   * this store has no reason to know the list or to be edited whenever it
   * changes. The page validates it against its own before using it.
   */
  lastTab?: string;
}

const STORED = "vibessh.applicationTabs";

/**
 * How many tabs to keep. Past this the strip stops being a shortcut and
 * becomes its own navigation problem, so the oldest is dropped - the same
 * thing a browser does when a window fills up, and less surprising than
 * refusing to open one.
 */
const MAX_TABS = 12;

function load(): ApplicationTab[] {
  try {
    const raw = window.localStorage.getItem(STORED);
    if (!raw) return [];
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    // Read defensively: this is storage an older version of the app wrote,
    // and a malformed entry must not take the whole strip with it.
    return parsed
      .filter((entry): entry is ApplicationTab => {
        return typeof entry === "object" && entry !== null && typeof (entry as ApplicationTab).id === "string";
      })
      .map((entry) => ({
        id: entry.id,
        name: typeof entry.name === "string" ? entry.name : entry.id,
        lastTab: typeof entry.lastTab === "string" ? entry.lastTab : undefined,
      }))
      .slice(0, MAX_TABS);
  } catch {
    // Private browsing, cleared storage, or a value this version cannot
    // read. An empty strip is a fine outcome.
    return [];
  }
}

function save(tabs: ApplicationTab[]): void {
  try {
    window.localStorage.setItem(STORED, JSON.stringify(tabs));
  } catch {
    // Storage unavailable or full. The tabs still work for this session.
  }
}

interface ApplicationTabsState {
  tabs: ApplicationTab[];
  /** Opening one that is already open renames it rather than duplicating it. */
  open: (tab: ApplicationTab) => void;
  /** Records which tab inside the Application is open, for the next visit. */
  rememberTab: (id: string, lastTab: string) => void;
  close: (id: string) => void;
  closeAll: () => void;
}

export const useApplicationTabsStore = create<ApplicationTabsState>((set) => ({
  tabs: load(),

  open: (tab) =>
    set((state) => {
      const existing = state.tabs.find((candidate) => candidate.id === tab.id);
      // A rename is worth picking up - the strip is the only place the name
      // is shown once you are inside the Application.
      // A rename must not forget where somebody was: `open` runs on every
      // visit, carrying only an id and a name.
      const tabs = existing
        ? state.tabs.map((candidate) => (candidate.id === tab.id ? { ...candidate, name: tab.name } : candidate))
        : [...state.tabs, tab].slice(-MAX_TABS);
      save(tabs);
      return { tabs };
    }),

  rememberTab: (id, lastTab) =>
    set((state) => {
      const existing = state.tabs.find((tab) => tab.id === id);
      // Unchanged, or an Application not in the strip at all: return the same
      // object so nothing subscribed to this store re-renders.
      if (!existing || existing.lastTab === lastTab) return state;
      const tabs = state.tabs.map((tab) => (tab.id === id ? { ...tab, lastTab } : tab));
      save(tabs);
      return { tabs };
    }),

  close: (id) =>
    set((state) => {
      const tabs = state.tabs.filter((tab) => tab.id !== id);
      save(tabs);
      return { tabs };
    }),

  closeAll: () => {
    save([]);
    return set({ tabs: [] });
  },
}));
