import type { SidebarGroup } from "@/types/common";

/**
 * The sidebar's real information architecture, grouped the same way the
 * production roadmap's target navigation is (Main / Workspace / Other today
 * - Infrastructure/Team/etc. join once Applications, Databases, and the
 * Team backend actually exist). Every entry here maps to a route that's
 * genuinely implemented; there's no placeholder/"coming soon" group for
 * features that don't exist yet, since a disabled link to nothing isn't
 * more honest than not showing it at all.
 */
export const sidebarGroups: SidebarGroup[] = [
  {
    id: "main",
    labelKey: "nav.groupMain",
    items: [
      { id: "dashboard", labelKey: "nav.dashboard", path: "/", icon: "layout-grid" },
      { id: "servers", labelKey: "nav.servers", path: "/servers", icon: "server" },
    ],
  },
  {
    id: "workspace",
    labelKey: "nav.groupWorkspace",
    items: [
      { id: "terminal", labelKey: "nav.terminal", path: "/terminal", icon: "terminal" },
      { id: "files", labelKey: "nav.files", path: "/files", icon: "folder" },
      { id: "monitor", labelKey: "nav.monitor", path: "/monitor", icon: "activity" },
      { id: "actions", labelKey: "nav.actions", path: "/actions", icon: "zap" },
    ],
  },
  {
    id: "other",
    labelKey: "nav.groupOther",
    items: [{ id: "settings", labelKey: "nav.settings", path: "/settings", icon: "settings" }],
  },
];
