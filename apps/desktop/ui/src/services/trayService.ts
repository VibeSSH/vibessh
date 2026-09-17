import { callCommand } from "./tauri";

/** What the close button does, and whether the explanation is still owed. */
export interface TraySettings {
  minimizeToTray: boolean;
  noticeShown: boolean;
}

export function getTraySettings(): Promise<TraySettings> {
  return callCommand<TraySettings>("get_tray_settings");
}

export function setMinimizeToTray(enabled: boolean): Promise<void> {
  return callCommand<void>("set_minimize_to_tray", { enabled });
}

/**
 * Tells the Rust side which language to build the tray menu in.
 *
 * The tray menu is the one user-visible text in VibeSSH that is not in the
 * i18n catalog: it exists while no webview is necessarily loaded, so it
 * cannot ask React anything. This is the wire between the language the
 * interface resolved and the words beside the clock.
 *
 * Deliberately fire-and-forget at the call sites. A tray menu left in the
 * previous language is a blemish; an interface that failed to start because
 * it could not rename a menu item would be a fault.
 */
export function setTrayLanguage(language: string): Promise<void> {
  return callCommand<void>("set_tray_language", { language });
}

/** Emitted by the tray's "Check for updates..." item - see `tray.rs`. */
export const TRAY_CHECK_FOR_UPDATES_EVENT = "tray://check-for-updates";
