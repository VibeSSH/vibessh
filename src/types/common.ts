export interface AppInfo {
  name: string;
  version: string;
}

export interface NavModule {
  id: string;
  label: string;
  path: string;
  icon: string;
  comingSoon?: boolean;
}
