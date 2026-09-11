//! Where VibeSSH is allowed to put working files on a managed Node.
//!
//! Every path here exists because the alternative - `/tmp` - turned out to
//! be wrong in the same two ways every time it was used:
//!
//! 1. **`/tmp` is world-writable**, so a predictable filename there can be
//!    pre-created by any local user as a symlink. Anything VibeSSH then
//!    writes through `sudo` follows that link and overwrites the target as
//!    root. `network::wireguard` had exactly this bug with a fixed
//!    `/tmp/vibessh-wg-strip.conf`.
//! 2. **`/tmp` is world-readable**, so a staging copy of a file, or a
//!    config containing a private key, is visible to every account on the
//!    Node - including the dedicated per-Application accounts that the
//!    whole isolation model exists to keep separated.
//!
//! `/run` is the right place for this: it is `tmpfs` (so nothing survives a
//! reboot, which is correct for every file here), it is root-owned, and
//! only root may create entries directly under a mode-0755 directory in it.
//!
//! **Why `BASE` is 0755 and not 0700.** 0700 would stop the connecting SSH
//! admin from even traversing into it, and one consumer genuinely needs
//! that: `runtime::docker`'s console FIFO is opened by the admin's own
//! shell (`exec 3<>`), not through `sudo`. 0755 still gives the property
//! that matters - **only root can create entries here**, so no unprivileged
//! user can plant a symlink - while leaving traversal open. Confidentiality
//! is then enforced per-entry by the entry's own mode, which is strictly
//! tighter: the WireGuard staging file is root-owned 0600, and the console
//! directory is 0700 owned by the connecting admin.

/// Root-owned, traversable, root-write-only. See the module doc for why
/// 0755 rather than 0700.
pub(crate) const BASE: &str = "/run/vibessh";

/// Per-Application console FIFOs (`runtime::docker`, `runtime::remote_process`).
/// Deliberately **not** inside the Application's own `working_directory`:
/// that directory is bind-mounted into the container and is readable by the
/// Application's own account, so a FIFO there is writable by the very thing
/// it controls the stdin of - and, when it was created world-writable, by
/// every *other* Application on the Node too.
pub(crate) const CONSOLE_DIR: &str = "/run/vibessh/console";

/// Creates `BASE` (root:root 0755) and `CONSOLE_DIR` (0700, owned by the
/// connecting admin, who is the identity that opens the FIFO). Idempotent -
/// `install -d` re-applies ownership and mode on a directory that already
/// exists, which also repairs a Node provisioned by an older build.
///
/// `$(id -un)`/`$(id -gn)` rather than a value passed in from the desktop:
/// the account that must own this is whichever identity the SSH session
/// actually authenticated as, and asking the remote side is the only way to
/// be sure that matches (a `Server` row's `username` can be stale, and
/// `sudo` may have changed the effective user).
pub(crate) fn ensure_runtime_dirs_command() -> String {
    format!(
        "sudo install -d -o root -g root -m 755 {BASE} && \
         sudo install -d -o \"$(id -un)\" -g \"$(id -gn)\" -m 700 {CONSOLE_DIR}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn console_dir_is_nested_under_the_base_directory() {
        assert!(CONSOLE_DIR.starts_with(&format!("{BASE}/")));
    }

    #[test]
    fn nothing_here_points_at_tmp() {
        // The whole reason this module exists.
        for path in [BASE, CONSOLE_DIR] {
            assert!(!path.starts_with("/tmp"), "{path}");
        }
    }

    #[test]
    fn the_setup_command_makes_the_base_root_owned_and_the_console_admin_owned() {
        let command = ensure_runtime_dirs_command();
        assert!(command.contains(&format!("install -d -o root -g root -m 755 {BASE}")), "{command}");
        assert!(command.contains(&format!("-m 700 {CONSOLE_DIR}")), "{command}");
        // The console directory must not be root-owned - the admin's own
        // shell opens the FIFO inside it without sudo.
        assert!(command.contains("-o \"$(id -un)\""), "{command}");
    }
}
