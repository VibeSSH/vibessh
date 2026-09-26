//! Sending email - for now only the password-reset code.
//!
//! Configured from the environment, like the TOTP key, and optional in the
//! same way: without it the service runs, and asking for a reset code is
//! refused with a reason rather than accepted and never delivered.
//!
//! - `SMTP_HOST`, `SMTP_USERNAME`, `SMTP_PASSWORD` - the mailbox it sends as.
//! - `MAIL_FROM` - the sender shown, e.g. `VibeSSH <noreply@vibessh.dev>`.
//! - `SMTP_PORT` - 465 (TLS from the start, the default) or 587 (STARTTLS).
//!
//! The password lives in the secrets file with the rest, never in the
//! repository, and is handed to the SMTP client directly - it is never part
//! of a command line (AGENTS.md section 2).

use std::sync::OnceLock;

use lettre::message::{header::ContentType, Mailbox};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

pub struct Mailer {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    from: Mailbox,
}

/// The configured mailer, or `None` when the environment does not name one.
pub fn mailer() -> Option<&'static Mailer> {
    static MAILER: OnceLock<Option<Mailer>> = OnceLock::new();
    MAILER
        .get_or_init(|| {
            let host = std::env::var("SMTP_HOST").ok().filter(|value| !value.trim().is_empty())?;
            let username = std::env::var("SMTP_USERNAME").ok()?;
            let password = std::env::var("SMTP_PASSWORD").ok()?;
            let from: Mailbox = match std::env::var("MAIL_FROM").ok()?.parse() {
                Ok(from) => from,
                Err(err) => {
                    log::warn!("MAIL_FROM isn't a valid sender ({err}) - email is unavailable");
                    return None;
                }
            };
            let port: u16 = std::env::var("SMTP_PORT").ok().and_then(|value| value.trim().parse().ok()).unwrap_or(465);
            let builder = if port == 465 {
                AsyncSmtpTransport::<Tokio1Executor>::relay(host.trim())
            } else {
                AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(host.trim())
            };
            match builder {
                Ok(builder) => Some(Mailer {
                    transport: builder.port(port).credentials(Credentials::new(username, password)).build(),
                    from,
                }),
                Err(err) => {
                    log::warn!("couldn't set up SMTP for {host}: {err} - email is unavailable");
                    None
                }
            }
        })
        .as_ref()
}

impl Mailer {
    pub async fn send(&self, to: &str, subject: &str, body: String) -> Result<(), String> {
        let to: Mailbox = to.parse().map_err(|err| format!("not an address: {err}"))?;
        let message = Message::builder()
            .from(self.from.clone())
            .to(to)
            .subject(subject)
            .header(ContentType::TEXT_PLAIN)
            .body(body)
            .map_err(|err| format!("couldn't build the message: {err}"))?;
        self.transport.send(message).await.map(|_| ()).map_err(|err| format!("the SMTP server refused it: {err}"))
    }
}

/// The reset email, in the language the app asked in - Polish unless it
/// asked for English.
pub fn password_reset_message(code: &str, language: &str) -> (&'static str, String) {
    if language == "en" {
        (
            "Your VibeSSH password reset code",
            format!(
                "Someone asked to reset the password of the VibeSSH account for this address.\n\n\
                 Your code: {code}\n\n\
                 Type it into VibeSSH together with your new password. It works once, for 30 minutes.\n\
                 If two-step verification is on, you will still need the code from your authenticator app to sign in.\n\n\
                 If this wasn't you, ignore this email - your password stays as it is.\n\n\
                 VibeSSH - https://vibessh.dev\n"
            ),
        )
    } else {
        (
            "Kod do zmiany hasła VibeSSH",
            format!(
                "Ktoś poprosił o zmianę hasła do konta VibeSSH przypisanego do tego adresu.\n\n\
                 Twój kod: {code}\n\n\
                 Wpisz go w VibeSSH razem z nowym hasłem. Działa raz, przez 30 minut.\n\
                 Jeśli masz włączoną weryfikację dwuetapową, przy logowaniu i tak podasz kod z aplikacji uwierzytelniającej.\n\n\
                 Jeśli to nie Ty, zignoruj tę wiadomość - hasło zostaje bez zmian.\n\n\
                 VibeSSH - https://vibessh.dev\n"
            ),
        )
    }
}
