export interface AppInfo {
  name: string;
  version: string;
}

export interface NavModule {
  id: string;
  labelKey: string;
  path: string;
  icon: string;
  comingSoon?: boolean;
}

export interface SidebarGroup {
  id: string;
  labelKey: string;
  items: NavModule[];
  /** Hidden entirely while signed out, rather than shown with links that would just bounce to a login prompt - see Sidebar.tsx. */
  requiresAuth?: boolean;
}
