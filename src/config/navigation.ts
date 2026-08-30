import type { NavModule } from "@/types/common";

/** Core pages available from Etap 1 onward. Settings isn't here - it moved
 * to its own icon in the Rail (see Rail.tsx), so it isn't duplicated
 * between two separate navigation surfaces. */
export const primaryNav: NavModule[] = [
  { id: "dashboard", label: "Dashboard", path: "/", icon: "layout-grid" },
  { id: "servers", label: "Servers", path: "/servers", icon: "server" },
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
  { id: "terminal", label: "Terminal", path: "/terminal", icon: "terminal" },
  { id: "files", label: "Files", path: "/files", icon: "folder" },
  { id: "monitor", label: "Monitor", path: "/monitor", icon: "activity" },
  { id: "actions", label: "Actions", path: "/actions", icon: "zap" },
  { id: "pro", label: "Pro", path: "/pro", icon: "sparkles", comingSoon: true },
];
