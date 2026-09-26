//! What somebody sees when VibeSSH cannot start, or dies.
//!
//! The release build aborts on panic (`panic = "abort"` in the workspace), and
//! Tauri turns an error from `setup` into a panic. Either way the window
//! simply vanished: no message, nothing in the log after the last normal
//! line, and the only trace a `0xc0000409` in the Windows event log. That is
//! how a database written by a newer build - which `storage::schema` answers
//! with a clear "update VibeSSH" - reached the person as "it opens and
//! closes", and the reason had to be dug out by hand.
//!
//! So both paths end here instead. Either way the error goes into the log and
//! a report file is written next to it - version, system, the error and the
//! log lines leading up to it - so whoever hit it can send us one file rather
//! than us having to find out what happened.
//!
//! A failed startup keeps the window: `setup` records the failure and
//! returns, and the interface asks for it (`get_startup_failure`) before
//! anything else and shows its own screen in place of the app - translated,
//! styled like the rest of VibeSSH, with the fix when there is a known one.
//!
//! A panic cannot do that - the process is already on its way out - so it
//! gets a native dialog, in Polish and English together, since there is no
//! way left to ask which language the person reads.

use crate::errors::{AppError, AppResult};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

/// Where the report goes. Known once `setup` has resolved the app's paths;
/// a panic before that writes to the temp directory instead.
static LOG_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Set while a report is being shown, so a panic inside the reporting
/// itself falls through to the default hook instead of recursing.
static REPORTING: AtomicBool = AtomicBool::new(false);

/// The file `tauri_plugin_log` writes to (`LogDir { file_name: None }` names
/// it after the product), whose tail goes into the report.
const LOG_FILE: &str = "VibeSSH.log";

/// How much of the log a report carries: enough to show what led up to the
/// failure, not so much the report is a chore to send.
const LOG_TAIL_LINES: usize = 60;

const DISCORD: &str = "discord.gg/CKAZWRJjJC";
const EMAIL: &str = "kryspekxd@gmail.com";
const SHOW_REPORT: &str = "Pokaż raport / Show report";
const CLOSE: &str = "Zamknij / Close";

/// Why the app did not start, for the interface's startup-failure screen.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupFailure {
    /// The error, shaped exactly as a failed command's is, so the interface
    /// translates it the same way.
    pub error: serde_json::Value,
    /// The report file's contents, for the screen's copy button.
    pub report: String,
    /// Where the report was written, if it could be.
    pub report_path: Option<PathBuf>,
}

static STARTUP_FAILURE: OnceLock<StartupFailure> = OnceLock::new();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// `setup` returned an error, so nothing the app needs was set up.
    Startup,
    /// A panic anywhere, at any time.
    Panic,
}

pub fn remember_log_dir(dir: PathBuf) {
    LOG_DIR.get_or_init(|| dir);
}

/// Replaces the default panic hook with one that reports the panic first.
/// The default hook still runs afterwards, so a terminal still gets the
/// usual message.
pub fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if !REPORTING.swap(true, Ordering::SeqCst) {
            let message = match (info.payload().downcast_ref::<&str>(), info.payload().downcast_ref::<String>()) {
                (Some(text), _) => (*text).to_string(),
                (_, Some(text)) => text.clone(),
                _ => "(no message)".to_string(),
            };
            let location = info.location().map(|at| format!("{}:{}", at.file(), at.line()));
            let (_, path) = write_report(Kind::Panic, &message, location.as_deref());
            if show_panic_dialog(&message, path.as_deref()) {
                if let Some(path) = &path {
                    reveal(path);
                }
            }
        }
        default_hook(info);
    }));
}

/// Records an error `setup` returned, for the interface to show. Called
/// instead of handing the error back to Tauri, which would only panic with it.
pub fn startup_failed(error: &(dyn std::error::Error + 'static)) {
    let message = error.to_string();
    let (report, report_path) = write_report(Kind::Startup, &message, None);
    let error = match error.downcast_ref::<AppError>() {
        Some(app_error) => serde_json::to_value(app_error),
        None => serde_json::to_value(AppError::Internal(message)),
    }
    .unwrap_or_default();
    STARTUP_FAILURE.get_or_init(|| StartupFailure { error, report, report_path });
}

/// Why the app did not start, if it did not.
pub fn startup_failure() -> Option<&'static StartupFailure> {
    STARTUP_FAILURE.get()
}

/// Opens the file manager on the startup failure's report.
pub fn reveal_startup_report() -> AppResult<()> {
    let path = startup_failure()
        .and_then(|failure| failure.report_path.as_deref())
        .ok_or_else(|| AppError::NotFound("no crash report was written".into()))?;
    reveal(path);
    Ok(())
}

/// Logs the failure and writes the report file; returns its text and, when
/// writing worked, its path.
fn write_report(kind: Kind, message: &str, location: Option<&str>) -> (String, Option<PathBuf>) {
    log::error!("{}: {message}{}", kind_label(kind), location.map(|at| format!(" (at {at})")).unwrap_or_default());
    log::logger().flush();

    let dir = LOG_DIR.get().cloned().unwrap_or_else(std::env::temp_dir);
    let log_tail = std::fs::read_to_string(dir.join(LOG_FILE)).map(|log| tail(&log, LOG_TAIL_LINES)).unwrap_or_default();
    let now = chrono::Local::now();
    let text = report_text(kind, message, location, &now.to_rfc3339(), &log_tail);
    let path = dir.join(format!("crash-{}.txt", now.format("%Y%m%d-%H%M%S")));
    match std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&path, &text)) {
        Ok(()) => (text, Some(path)),
        Err(err) => {
            log::warn!("couldn't write the crash report to {}: {err}", path.display());
            (text, None)
        }
    }
}

fn kind_label(kind: Kind) -> &'static str {
    match kind {
        Kind::Startup => "VibeSSH could not start",
        Kind::Panic => "VibeSSH hit an unexpected error",
    }
}

/// The report file's contents - everything we would otherwise have to ask
/// for, in the order we would ask for it.
fn report_text(kind: Kind, message: &str, location: Option<&str>, time: &str, log_tail: &str) -> String {
    let mut text = format!(
        "{}

Version: {}
System:  {} {}
Time:    {time}
",
        kind_label(kind),
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
    );
    if let Some(at) = location {
        text.push_str(&format!("Where:   {at}
"));
    }
    text.push_str(&format!("
{message}
"));
    if !log_tail.is_empty() {
        text.push_str(&format!("
--- The last lines of the log ---
{log_tail}
"));
    }
    text
}

fn tail(text: &str, lines: usize) -> String {
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

/// The dialog a panic gets; true when the person asked to see the report.
fn show_panic_dialog(message: &str, report: Option<&Path>) -> bool {
    let (pl_headline, en_headline) = (
        "VibeSSH napotkał nieoczekiwany błąd i musi się zamknąć.",
        "VibeSSH hit an unexpected error and has to close.",
    );
    let (pl_send, en_send) = match report {
        Some(_) => (
            "Kliknij „Pokaż raport” i wyślij nam ten plik",
            "Click \"Show report\" and send us that file",
        ),
        None => ("Wyślij nam zrzut ekranu tego okna", "Send us a screenshot of this window"),
    };
    let description = format!(
        "{pl_headline}\n\n{message}\n\n{pl_send} - na Discordzie ({DISCORD}) albo na {EMAIL}. Dzięki temu szybko to naprawimy.\n\n\
         {en_headline}\n\n{en_send} - on Discord ({DISCORD}) or to {EMAIL}, and we'll get it fixed."
    );
    let buttons = match report {
        Some(_) => rfd::MessageButtons::OkCancelCustom(SHOW_REPORT.into(), CLOSE.into()),
        None => rfd::MessageButtons::OkCustom(CLOSE.into()),
    };
    let result = rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title("VibeSSH")
        .set_description(description)
        .set_buttons(buttons)
        .show();
    report.is_some()
        && match result {
            rfd::MessageDialogResult::Ok => true,
            rfd::MessageDialogResult::Custom(label) => label == SHOW_REPORT,
            _ => false,
        }
}

/// Opens the file manager on the report, selected where the platform allows
/// it, so it can be dragged straight into a message.
fn reveal(path: &Path) {
    #[cfg(windows)]
    let opened = std::process::Command::new("explorer").arg(format!("/select,{}", path.display())).spawn();
    #[cfg(target_os = "macos")]
    let opened = std::process::Command::new("open").arg("-R").arg(path).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let opened = std::process::Command::new("xdg-open").arg(path.parent().unwrap_or(path)).spawn();
    if let Err(err) = opened {
        log::warn!("couldn't open the file manager on {}: {err}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_report_carries_what_we_would_ask_for() {
        let text = report_text(
            Kind::Startup,
            "this server database was created by a newer version of VibeSSH",
            None,
            "2026-09-26T14:59:06+02:00",
            "a log line",
        );

        assert!(text.starts_with("VibeSSH could not start\n"));
        assert!(text.contains(&format!("Version: {}", env!("CARGO_PKG_VERSION"))));
        assert!(text.contains("Time:    2026-09-26T14:59:06+02:00"));
        assert!(text.contains("created by a newer version"));
        assert!(text.contains("--- The last lines of the log ---\na log line"));
        assert!(!text.contains("Where:"));
    }

    #[test]
    fn a_panic_report_says_where() {
        let text = report_text(Kind::Panic, "boom", Some("src/lib.rs:12"), "now", "");

        assert!(text.contains("Where:   src/lib.rs:12"));
        assert!(!text.contains("The last lines of the log"));
    }

    #[test]
    fn the_log_tail_keeps_the_last_lines() {
        let log: String = (1..=100).map(|n| format!("line {n}\n")).collect();

        let kept = tail(&log, 3);

        assert_eq!(kept, "line 98\nline 99\nline 100");
        assert_eq!(tail("only\n", 60), "only");
    }
}
