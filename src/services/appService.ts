import { callCommand } from "./tauri";
import type { AppInfo } from "@/types/common";

export function getAppInfo(): Promise<AppInfo> {
  return callCommand<AppInfo>("get_app_info");
}
