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

use lettre::message::{Mailbox, MultiPart};
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


/// One email, in both forms: the HTML a mail client shows, and the plain
/// text it falls back on - which is also what a spam filter expects to find
/// beside HTML, so it is always sent.
pub struct Email {
    pub subject: &'static str,
    pub text: String,
    pub html: String,
}

impl Mailer {
    pub async fn send(&self, to: &str, email: &Email) -> Result<(), String> {
        let to: Mailbox = to.parse().map_err(|err| format!("not an address: {err}"))?;
        let message = Message::builder()
            .from(self.from.clone())
            .to(to)
            .subject(email.subject)
            .multipart(MultiPart::alternative_plain_html(email.text.clone(), email.html.clone()))
            .map_err(|err| format!("couldn't build the message: {err}"))?;
        self.transport.send(message).await.map(|_| ()).map_err(|err| format!("the SMTP server refused it: {err}"))
    }
}

/// The words of the reset email in one language.
struct ResetWords {
    subject: &'static str,
    heading: &'static str,
    intro: &'static str,
    code_label: &'static str,
    instructions: &'static str,
    two_factor: &'static str,
    not_you: &'static str,
}

const RESET_PL: ResetWords = ResetWords {
    subject: "Kod do zmiany hasła VibeSSH",
    heading: "Zmiana hasła",
    intro: "Ktoś poprosił o zmianę hasła do konta VibeSSH przypisanego do tego adresu.",
    code_label: "Twój kod",
    instructions: "Wpisz go w VibeSSH razem z nowym hasłem. Działa raz, przez 30 minut.",
    two_factor: "Jeśli masz włączoną weryfikację dwuetapową, przy logowaniu i tak podasz kod z aplikacji uwierzytelniającej.",
    not_you: "Jeśli to nie Ty, zignoruj tę wiadomość - hasło zostaje bez zmian.",
};

const RESET_EN: ResetWords = ResetWords {
    subject: "Your VibeSSH password reset code",
    heading: "Reset your password",
    intro: "Someone asked to reset the password of the VibeSSH account for this address.",
    code_label: "Your code",
    instructions: "Type it into VibeSSH together with your new password. It works once, for 30 minutes.",
    two_factor: "If two-step verification is on, you will still need the code from your authenticator app to sign in.",
    not_you: "If this wasn't you, ignore this email - your password stays as it is.",
};

/// The reset email, in the language the app asked in - Polish unless it
/// asked for English.
///
/// The HTML is built for mail clients, which is its own discipline: a table
/// for layout, every style inline, no web fonts or scripts, and the one image
/// loaded from the site. The code is the only thing in the accent colour.
/// Everything put into it is fixed text or the code, whose alphabet has
/// nothing HTML would read as markup.
pub fn password_reset_message(code: &str, language: &str) -> Email {
    let words = if language == "en" { &RESET_EN } else { &RESET_PL };
    let text = format!(
        "{intro}\n\n{code_label}: {code}\n\n{instructions}\n{two_factor}\n\n{not_you}\n\nVibeSSH - https://vibessh.dev\n",
        intro = words.intro,
        code_label = words.code_label,
        instructions = words.instructions,
        two_factor = words.two_factor,
        not_you = words.not_you,
    );
    let html = format!(
        r##"<!doctype html>
<html lang="{lang}">
<head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>{subject}</title></head>
<body style="margin:0;padding:0;background:#f3f4f6;">
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="background:#f3f4f6;padding:32px 12px;">
  <tr><td align="center">
    <table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="max-width:520px;background:#ffffff;border-radius:14px;overflow:hidden;border:1px solid #e5e7eb;">
      <tr><td style="background:#0d1117;padding:22px 28px;">
        <table role="presentation" cellpadding="0" cellspacing="0"><tr>
          <td style="vertical-align:middle;"><img src="https://vibessh.dev/vibessh-mark.png" width="28" height="28" alt="" style="display:block;border:0;"></td>
          <td style="vertical-align:middle;padding-left:10px;font-family:-apple-system,Segoe UI,Roboto,Helvetica,Arial,sans-serif;font-size:17px;font-weight:600;color:#ecedef;">VibeSSH</td>
        </tr></table>
      </td></tr>
      <tr><td style="padding:30px 28px 4px;font-family:-apple-system,Segoe UI,Roboto,Helvetica,Arial,sans-serif;">
        <h1 style="margin:0 0 12px;font-size:21px;font-weight:600;color:#0d1117;">{heading}</h1>
        <p style="margin:0 0 22px;font-size:15px;line-height:1.55;color:#424a53;">{intro}</p>
        <p style="margin:0 0 8px;font-size:12px;font-weight:600;letter-spacing:.06em;text-transform:uppercase;color:#6e7781;">{code_label}</p>
        <div style="margin:0 0 22px;padding:16px 18px;background:#0d1117;border-radius:10px;font-family:Consolas,Menlo,monospace;font-size:26px;font-weight:600;letter-spacing:.14em;color:#4dd9f5;text-align:center;">{code}</div>
        <p style="margin:0 0 10px;font-size:14px;line-height:1.55;color:#424a53;">{instructions}</p>
        <p style="margin:0 0 22px;font-size:14px;line-height:1.55;color:#424a53;">{two_factor}</p>
        <p style="margin:0 0 26px;padding-top:18px;border-top:1px solid #e5e7eb;font-size:13px;line-height:1.55;color:#6e7781;">{not_you}</p>
      </td></tr>
    </table>
    <p style="margin:16px 0 0;font-family:-apple-system,Segoe UI,Roboto,Helvetica,Arial,sans-serif;font-size:12px;"><a href="https://vibessh.dev" style="color:#8c959f;text-decoration:none;">vibessh.dev</a></p>
  </td></tr>
</table>
</body>
</html>
"##,
        lang = if language == "en" { "en" } else { "pl" },
        subject = words.subject,
        heading = words.heading,
        intro = words.intro,
        code_label = words.code_label,
        instructions = words.instructions,
        two_factor = words.two_factor,
        not_you = words.not_you,
    );
    Email { subject: words.subject, text, html }
}
