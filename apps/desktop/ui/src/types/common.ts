export interface AppInfo {
  name: string;
  version: string;
  /**
   * Running as root on Linux, which quietly breaks every stored secret -
   * root cannot see the user's keyring. Always false on other platforms.
   */
  runningAsRoot: boolean;
}

export interface NavModule {
  id: string;
  labelKey: string;
  path: string;
  icon: string;
  comingSoon?: boolean;
  /** Hidden entirely while signed out, rather than shown with a link that would just bounce to a login prompt - see Sidebar.tsx. Item-level so a group can mix signed-in-only and always-visible entries (e.g. "Security" holding both Firewall and Team). */
  requiresAuth?: boolean;
}

export interface SidebarGroup {
  id: string;
  labelKey: string;
  items: NavModule[];
  /** Hidden entirely while signed out, rather than shown with links that would just bounce to a login prompt - see Sidebar.tsx. */
  requiresAuth?: boolean;
}
