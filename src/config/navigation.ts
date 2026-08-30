import type { NavModule } from "@/types/common";

/** Core pages available from Etap 1 onward. */
export const primaryNav: NavModule[] = [
  { id: "dashboard", label: "Dashboard", path: "/", icon: "layout-grid" },
  { id: "servers", label: "Servers", path: "/servers", icon: "server" },
  { id: "settings", label: "Settings", path: "/settings", icon: "settings" },
];

/**
 * Per-server modules. These map 1:1 to the VibeSSH product surface but are
 * implemented in later stages (SSH in Etap 3, SFTP/monitoring/actions after).
 * Listed now so the shell and branding are in place before the features land.
 */
export const moduleNav: NavModule[] = [
  { id: "terminal", label: "VibeSSH Terminal", path: "/terminal", icon: "terminal", comingSoon: true },
  { id: "files", label: "VibeSSH Files", path: "/files", icon: "folder", comingSoon: true },
  { id: "monitor", label: "VibeSSH Monitor", path: "/monitor", icon: "activity", comingSoon: true },
  { id: "actions", label: "VibeSSH Actions", path: "/actions", icon: "zap", comingSoon: true },
  { id: "pro", label: "VibeSSH Pro", path: "/pro", icon: "sparkles", comingSoon: true },
];
