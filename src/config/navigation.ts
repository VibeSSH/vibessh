import type { SidebarGroup } from "@/types/common";

/**
 * The sidebar's real information architecture. Every entry here maps to a
 * route that's genuinely implemented; there's no placeholder/"coming soon"
 * group for features that don't exist yet, since a disabled link to
 * nothing isn't more honest than not showing it at all.
 *
 * Deliberately does NOT have its own "Performance"/"Wydajność" group even
 * though Monitor is a metrics screen - a lone-item category is worse than
 * folding it into Tools, and per-Application resource usage already lives
 * contextually on the Application's own detail page (Resource Limits /
 * usage card), not as a second global nav entry for the same concept.
 */
export const sidebarGroups: SidebarGroup[] = [
  {
    id: "main",
    labelKey: "nav.groupMain",
    items: [
      { id: "dashboard", labelKey: "nav.dashboard", path: "/", icon: "layout-grid" },
      { id: "servers", labelKey: "nav.servers", path: "/servers", icon: "server" },
      { id: "applications", labelKey: "nav.applications", path: "/applications", icon: "box" },
    ],
  },
  {
    id: "infrastructure",
    labelKey: "nav.groupInfrastructure",
    items: [
      { id: "vibe-network", labelKey: "nav.vibeNetwork", path: "/vibe-network", icon: "wifi" },
      { id: "database-hosts", labelKey: "nav.databaseHosts", path: "/database-hosts", icon: "database" },
    ],
  },
  {
    id: "tools",
    labelKey: "nav.groupTools",
    items: [
      { id: "terminal", labelKey: "nav.terminal", path: "/terminal", icon: "terminal" },
      { id: "files", labelKey: "nav.files", path: "/files", icon: "folder" },
      { id: "monitor", labelKey: "nav.monitor", path: "/monitor", icon: "activity" },
      { id: "actions", labelKey: "nav.actions", path: "/actions", icon: "zap" },
      { id: "port-forwarding", labelKey: "nav.portForwarding", path: "/port-forwarding", icon: "arrow-left-right" },
      { id: "vibe-ai", labelKey: "nav.vibeAi", path: "/vibe-ai", icon: "sparkles" },
    ],
  },
  {
    id: "security",
    labelKey: "nav.groupSecurity",
    items: [
      { id: "firewall", labelKey: "nav.firewall", path: "/firewall", icon: "lock" },
      { id: "teams", labelKey: "nav.teams", path: "/teams", icon: "users", requiresAuth: true },
    ],
  },
  {
    id: "other",
    labelKey: "nav.groupOther",
    items: [
      { id: "guide", labelKey: "nav.guide", path: "/guide", icon: "book-open" },
      { id: "settings", labelKey: "nav.settings", path: "/settings", icon: "settings" },
    ],
  },
];
