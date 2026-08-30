import type { NavModule } from "@/types/common";

/** Core pages available from Etap 1 onward. */
export const primaryNav: NavModule[] = [
  { id: "dashboard", label: "Dashboard", path: "/", icon: "layout-grid" },
  { id: "servers", label: "Servers", path: "/servers", icon: "server" },
  { id: "settings", label: "Settings", path: "/settings", icon: "settings" },
];

/**
 * Per-server modules. Each is real (SSH transport, SFTP, monitoring,
 * systemd/Docker actions), but every one of them needs a specific server to
 * act on - the sidebar link lands on a server picker (see
 * `pages/ModulePicker.tsx`), not the module directly, since there's no
 * "current server" concept outside of that. `pro` is the only one still
 * genuinely unbuilt.
 */
export const moduleNav: NavModule[] = [
  { id: "terminal", label: "VibeSSH Terminal", path: "/terminal", icon: "terminal" },
  { id: "files", label: "VibeSSH Files", path: "/files", icon: "folder" },
  { id: "monitor", label: "VibeSSH Monitor", path: "/monitor", icon: "activity" },
  { id: "actions", label: "VibeSSH Actions", path: "/actions", icon: "zap" },
  { id: "pro", label: "VibeSSH Pro", path: "/pro", icon: "sparkles", comingSoon: true },
];
