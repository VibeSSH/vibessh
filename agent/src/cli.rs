//! `vibe-agent pair <code>` - a short blocking CLI call into the already
//! running daemon's local control endpoint. Deliberately synchronous (no
//! tokio runtime spun up just to make one HTTP request) since this is a
//! separate, short-lived process invocation, not part of the daemon.

use serde::Deserialize;

use crate::DEFAULT_CONTROL_BIND;

#[derive(Deserialize)]
struct PairResponse {
    ok: bool,
    message: String,
}

pub fn pair(code: &str) {
    if code.is_empty() {
        eprintln!("usage: vibe-agent pair <CODE>");
        std::process::exit(2);
    }

    let control_addr =
        std::env::var("VIBESSH_AGENT_CONTROL_BIND").unwrap_or_else(|_| DEFAULT_CONTROL_BIND.to_string());
    let url = format!("http://{control_addr}/internal/pair");

    let result = ureq::post(&url).send_json(ureq::json!({ "code": code }));

    match result {
        Ok(response) => match response.into_json::<PairResponse>() {
            Ok(body) if body.ok => {
                println!("{}", body.message);
            }
            Ok(body) => {
                eprintln!("pairing failed: {}", body.message);
                std::process::exit(1);
            }
            Err(err) => {
                eprintln!("pairing failed: could not parse the agent's response ({err})");
                std::process::exit(1);
            }
        },
        Err(err) => {
            eprintln!(
                "could not reach the local vibe-agent daemon at {control_addr}: {err}\n\
                 Is `vibe-agent` running as a service on this machine?"
            );
            std::process::exit(1);
        }
    }
}
