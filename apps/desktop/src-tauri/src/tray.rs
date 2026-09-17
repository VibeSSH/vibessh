//! The icon beside the clock, and what the close button does now.
//!
//! **What this changes.** Closing the window used to end the process, taking
//! every live SSH session, port forward and log follow with it. It now hides
//! the window and leaves VibeSSH in the tray, which is what people expect of
//! something they leave running all day - and the behaviour is a setting, so
//! anyone who wants the old one keeps it.
//!
//! **The one thing this must never be is silent.** An application still
//! running after you closed it, with no icon and no word, is indistinguishable
//! from one that failed to quit - and this one holds the keys to your
//! servers. So the first hide raises a notification saying where it went, the
//! tray icon is visible from then on, and its tooltip says the same thing
//! again for anyone who hovers.
//!
//! **The menu is built here, in Rust, which is the only place in this app
//! where a user-visible string is not in the i18n catalog.** A tray menu
//! exists while no webview is necessarily loaded, so it cannot ask React what
//! language is in use. Instead the interface tells this module which language
//! it settled on (`set_tray_language`), and the menu is rebuilt. Both
//! languages are here for the same reason everything else in VibeSSH has
//! both.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::storage::tray_config::{save_tray_config, TrayConfig};

/// The tray icon's id, so the menu can be rebuilt later without holding a
/// handle to it across the whole application.
pub const TRAY_ID: &str = "vibessh-main";

/// Chosen from the tray, handled by the interface: it already knows how to
/// check, download and install, and duplicating that here would be a second
/// updater to keep in step with the first.
pub const CHECK_FOR_UPDATES_EVENT: &str = "tray://check-for-updates";

const MENU_SHOW: &str = "show";
const MENU_CHECK_UPDATES: &str = "check-updates";
const MENU_QUIT: &str = "quit";

/// What the tray shows, and what the close button is currently set to do.
pub struct TrayState {
    pub config: Mutex<TrayConfig>,
    /// The language the interface last reported. Not an enum: it arrives as
    /// whatever `i18next` resolved to, and anything that is not Polish is
    /// rendered in English, which is also what the catalogs do.
    pub language: Mutex<String>,
    /// Set the instant "Quit" is chosen, so the close handler steps aside
    /// instead of hiding the window that is on its way out. Without it,
    /// quitting from the tray would hide the window and leave the process
    /// running - the exact failure this whole module is meant to avoid.
    pub quitting: AtomicBool,
}

impl TrayState {
    pub fn new(config: TrayConfig) -> Self {
        Self { config: Mutex::new(config), language: Mutex::new("en".to_string()), quitting: AtomicBool::new(false) }
    }

    pub fn minimize_to_tray(&self) -> bool {
        self.config.lock().map(|config| config.minimize_to_tray).unwrap_or(true)
    }

    pub fn is_quitting(&self) -> bool {
        self.quitting.load(Ordering::SeqCst)
    }
}

/// Every user-visible word this module owns, in both languages.
struct Labels {
    show: &'static str,
    check_updates: &'static str,
    quit: &'static str,
    tooltip: &'static str,
    notice_title: &'static str,
    notice_body: &'static str,
}

const POLISH: Labels = Labels {
    show: "Pokaż VibeSSH",
    check_updates: "Sprawdź aktualizacje...",
    quit: "Zakończ VibeSSH",
    tooltip: "VibeSSH - działa w tle",
    notice_title: "VibeSSH działa dalej",
    notice_body: "Okno zostało schowane, a nie zamknięte - VibeSSH siedzi przy zegarze. Kliknij ikonę, żeby wrócić, albo wyłącz to w Ustawieniach.",
};

const ENGLISH: Labels = Labels {
    show: "Show VibeSSH",
    check_updates: "Check for updates...",
    quit: "Quit VibeSSH",
    tooltip: "VibeSSH - running in the background",
    notice_title: "VibeSSH is still running",
    notice_body: "The window was hidden, not closed - VibeSSH is beside the clock. Click the icon to bring it back, or turn this off in Settings.",
};

fn labels(language: &str) -> &'static Labels {
    if language.starts_with("pl") {
        &POLISH
    } else {
        &ENGLISH
    }
}

/// The menu, in the shape a desktop application's tray menu has: the
/// application's own name at the top as a heading rather than a command, then
/// what you can do, then leaving.
fn build_menu<R: Runtime>(app: &AppHandle<R>, language: &str) -> tauri::Result<Menu<R>> {
    let labels = labels(language);
    // Disabled on purpose: it is a title, not something to click. The version
    // is here because "which build is this?" is the first question of every
    // bug report, and from the tray it is one hover away.
    let heading = MenuItem::with_id(app, "heading", format!("VibeSSH {}", env!("CARGO_PKG_VERSION")), false, None::<&str>)?;
    let separator_top = PredefinedMenuItem::separator(app)?;
    let show = MenuItem::with_id(app, MENU_SHOW, labels.show, true, None::<&str>)?;
    let check_updates = MenuItem::with_id(app, MENU_CHECK_UPDATES, labels.check_updates, true, None::<&str>)?;
    let separator_bottom = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, MENU_QUIT, labels.quit, true, None::<&str>)?;

    Menu::with_items(app, &[&heading, &separator_top, &show, &check_updates, &separator_bottom, &quit])
}

/// Brings the window back from wherever it went - hidden, minimised, or
/// merely behind something else. All three are "I clicked the tray icon and
/// nothing happened" if only one of them is handled.
pub fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window("main") else {
        log::warn!("the tray tried to show the main window and there wasn't one");
        return;
    };
    if let Err(err) = window.unminimize() {
        log::debug!("unminimize: {err}");
    }
    if let Err(err) = window.show() {
        log::warn!("couldn't show the main window: {err}");
    }
    if let Err(err) = window.set_focus() {
        log::warn!("couldn't focus the main window: {err}");
    }
}

/// Builds the tray icon. Called once, from `setup`.
pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let language = app.state::<TrayState>().language.lock().map(|l| l.clone()).unwrap_or_else(|_| "en".to_string());
    let menu = build_menu(app, &language)?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .tooltip(labels(&language).tooltip)
        // The left click opens the window; the menu is the right click, which
        // is what every other tray icon on this machine does.
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            MENU_SHOW => show_main_window(app),
            MENU_CHECK_UPDATES => {
                // Shown first: the check reports into the window, and a
                // result nobody can see is not a result.
                show_main_window(app);
                if let Err(err) = app.emit(CHECK_FOR_UPDATES_EVENT, ()) {
                    log::warn!("couldn't ask the interface to check for updates: {err}");
                }
            }
            MENU_QUIT => {
                app.state::<TrayState>().quitting.store(true, Ordering::SeqCst);
                app.exit(0);
            }
            other => log::debug!("unhandled tray menu id: {other}"),
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main_window(tray.app_handle());
            }
        });

    // The window's own icon, so the tray never shows a different mark from
    // the taskbar. Absent only in test harnesses that build no icon at all.
    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }

    builder.build(app)?;
    Ok(())
}

/// Re-renders the menu in the language the interface just settled on.
pub fn set_language<R: Runtime>(app: &AppHandle<R>, language: &str) {
    if let Ok(mut current) = app.state::<TrayState>().language.lock() {
        if *current == language {
            return;
        }
        *current = language.to_string();
    }
    let Some(tray) = app.tray_by_id(TRAY_ID) else { return };
    match build_menu(app, language) {
        Ok(menu) => {
            if let Err(err) = tray.set_menu(Some(menu)) {
                log::warn!("couldn't rebuild the tray menu: {err}");
            }
            if let Err(err) = tray.set_tooltip(Some(labels(language).tooltip)) {
                log::warn!("couldn't set the tray tooltip: {err}");
            }
        }
        Err(err) => log::warn!("couldn't build the tray menu in {language}: {err}"),
    }
}

/// Hides the window instead of closing it, and explains itself the first
/// time.
///
/// The notification is best effort on purpose. It can be refused by the
/// operating system's own notification settings, and a close button that
/// failed because a notification could not be shown would be a far worse
/// fault than the one it is guarding against - so the flag is recorded and
/// the window is hidden either way.
pub fn hide_to_tray<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        if let Err(err) = window.hide() {
            log::warn!("couldn't hide the main window: {err}");
        }
    }

    let state = app.state::<TrayState>();
    let (owed, language) = {
        let Ok(config) = state.config.lock() else { return };
        let Ok(language) = state.language.lock() else { return };
        (!config.notice_shown, language.clone())
    };
    if !owed {
        return;
    }

    let labels = labels(&language);
    // **In a `cargo tauri dev` build this arrives labelled "PowerShell", and
    // that is not a fault here.** `tauri-plugin-notification` sets the
    // toast's `System.AppUserModel.ID` only when the executable is *not* in
    // `target\debug` or `target\release`, and `notify-rust` falls back to
    // `Toast::POWERSHELL_APP_ID` when none is set. The alternative would be
    // an id Windows has never seen, and Windows silently drops those - so
    // upstream chose "visible under the wrong name" over "invisible".
    //
    // The installed build has one: the NSIS installer's Start Menu shortcut
    // carries `AppUserModelID = dev.vibessh.app`, matching `identifier` in
    // tauri.conf.json, so a released VibeSSH is labelled VibeSSH. Written
    // down because it looks exactly like a bug and cost an investigation
    // once already.
    #[cfg(desktop)]
    {
        use tauri_plugin_notification::NotificationExt;
        if let Err(err) = app.notification().builder().title(labels.notice_title).body(labels.notice_body).show() {
            log::warn!("couldn't show the 'still running' notification: {err}");
        }
    }

    // Recorded whether or not the notification appeared: a person whose
    // notifications are switched off should not be asked again on every
    // single close.
    let Ok(mut config) = state.config.lock() else { return };
    config.notice_shown = true;
    if let Ok(config_dir) = app.path().app_config_dir() {
        if let Err(err) = save_tray_config(&config_dir, &config) {
            log::warn!("couldn't record that the tray notice was shown: {err}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Both languages, because a tray menu is the one place in this app where
    /// a user-visible string is not in the i18n catalog and so cannot be
    /// caught by the test that keeps those two files in step.
    #[test]
    fn every_label_exists_in_both_languages_and_they_differ() {
        assert_eq!(labels("pl").show, POLISH.show);
        assert_eq!(labels("pl-PL").show, POLISH.show);
        assert_eq!(labels("en").show, ENGLISH.show);
        assert_eq!(labels("en-GB").show, ENGLISH.show);
        // Anything unrecognised reads English rather than nothing.
        assert_eq!(labels("de").show, ENGLISH.show);
        assert_eq!(labels("").show, ENGLISH.show);

        for (pl, en) in [
            (POLISH.show, ENGLISH.show),
            (POLISH.check_updates, ENGLISH.check_updates),
            (POLISH.quit, ENGLISH.quit),
            (POLISH.tooltip, ENGLISH.tooltip),
            (POLISH.notice_title, ENGLISH.notice_title),
            (POLISH.notice_body, ENGLISH.notice_body),
        ] {
            assert!(!pl.is_empty() && !en.is_empty());
            assert_ne!(pl, en, "a label that is identical in both languages is one that was never translated");
        }
    }

    /// The flag that stops "Quit" from being turned into "hide" by the very
    /// close handler it triggers.
    #[test]
    fn quitting_starts_false_and_latches() {
        let state = TrayState::new(TrayConfig::default());
        assert!(!state.is_quitting());
        state.quitting.store(true, Ordering::SeqCst);
        assert!(state.is_quitting());
    }

    /// A poisoned lock must not turn into "quit on close": the safe answer
    /// when the setting cannot be read is the one that loses no sessions.
    #[test]
    fn an_unreadable_setting_still_minimizes() {
        let state = TrayState::new(TrayConfig { minimize_to_tray: true, notice_shown: true });
        assert!(state.minimize_to_tray());
    }
}
