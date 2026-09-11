//! Passwords held for as long as the app is running, and no longer.
//!
//! **Why this exists.** `storage::credentials` keeps secrets in the OS
//! credential store and refuses to write them anywhere else - a config file is
//! readable by anything running as this user, ends up in backups, and turns up
//! in screenshots. That refusal is right, and it leaves people on a machine
//! with no working Secret Service unable to use password authentication at
//! all. Two of the first Linux users hit exactly that.
//!
//! So a password can live here instead: in memory, for this run. Nothing is
//! written, nothing survives the process, and the next start asks again. That
//! is a worse experience than a keyring and a better one than either of the
//! alternatives - refusing to connect, or writing the password to a file.
//!
//! **This is not a cache in front of the keyring.** A password that the
//! keyring already holds never comes through here; this only fills the gap
//! where there is no keyring to hold it.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use uuid::Uuid;

/// Process-wide rather than Tauri-managed state.
///
/// The one place that reads it - `credentials_from_server` - sits several
/// layers below the command that would hold a `State<...>`, and threading a
/// handle down every connection path would touch a great deal of code to
/// deliver something that has no configuration and exactly one instance.
/// `runtime::local_docker_console` is process-wide for the same reason.
fn passwords() -> &'static Mutex<HashMap<Uuid, String>> {
    static PASSWORDS: OnceLock<Mutex<HashMap<Uuid, String>>> = OnceLock::new();
    PASSWORDS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn remember(server_id: Uuid, password: String) {
    if let Ok(mut map) = passwords().lock() {
        map.insert(server_id, password);
    }
}

pub fn get(server_id: Uuid) -> Option<String> {
    passwords().lock().ok()?.get(&server_id).cloned()
}

/// Called when a Node is deleted, and when a password turns out to be wrong -
/// keeping a rejected one would mean the prompt never comes back and every
/// later attempt fails the same way.
pub fn forget(server_id: Uuid) {
    if let Ok(mut map) = passwords().lock() {
        map.remove(&server_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_remembered_password_comes_back_and_a_forgotten_one_does_not() {
        // Fresh ids per test rather than a fresh store - the store is
        // process-wide, so the tests share one and must not share entries.
        let id = Uuid::new_v4();

        remember(id, "hunter2".into());
        assert_eq!(get(id).as_deref(), Some("hunter2"));

        forget(id);
        assert_eq!(get(id), None);
    }

    #[test]
    fn nodes_do_not_share_one() {
        remember(Uuid::new_v4(), "hunter2".into());

        assert_eq!(get(Uuid::new_v4()), None);
    }
}
