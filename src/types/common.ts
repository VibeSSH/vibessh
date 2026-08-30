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
}
