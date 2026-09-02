//! `SudoUserApplicationFileProvider` - `ApplicationFileProvider` for a
//! Docker Application that opted into
//! `runtime::docker::DockerConfig::run_as_dedicated_user` (see
//! `crate::dedicated_user`'s own doc comment for why: a dedicated,
//! unprivileged Linux account per Application instead of the shared
//! connecting SSH admin, so one Application's files stay genuinely
//! inaccessible to every other one).
//!
//! **No new SSH session, no SFTP identity switch.** SFTP's whole subsystem
//! channel is tied to one authenticated identity for its entire lifetime,
//! the connecting admin - there's no way to make one such channel act as a
//! different Linux user mid-session. Instead, every operation shells out
//! through the *existing* admin connection to a single, fixed, root-owned
//! helper script (`HELPER_SCRIPT`/`ensure_helper_installed`) via
//! `sudo -u <dedicated user> <helper> <root> <op> <args...>`.
//!
//! **Why a fixed helper script instead of N ad hoc shell commands** (the
//! pattern every other SSH-touching feature in this codebase already uses,
//! e.g. `runtime::docker::build_create_command`): this crosses a real
//! privilege boundary - a lower-privileged, per-Application identity -
//! rather than just running more commands as the already-fully-trusted
//! connecting admin, so the standing bar here is higher. One small, fixed,
//! reviewable allowlist of operations, installed once and never rebuilt per
//! call, each one independently re-validating its own target stays inside
//! the Application's own root (defense in depth, on top of the identical
//! check this module's own `resolve` already does before ever invoking it),
//! rather than ten different per-operation command strings each needing its
//! own escaping/containment logic gotten right on its own.
//!
//! **Binary-safe content never touches a shell argument or command
//! string.** `read_file`/`write_file`/`download_file`/`upload_file` stage
//! bytes through a `/tmp` file instead: the connecting admin's own
//! already-proven SFTP transfer (`SshSession::read_file`/`write_file`/
//! `download_file_with_progress`/`upload_file_with_progress`) moves the
//! actual bytes, and the helper's `read`/`write` operations only ever `cp`
//! between that staging path and the real target - the privilege crossing
//! and the byte transfer are two separate, independently simple steps
//! rather than one operation trying to do both.

use std::path::Path;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use vibessh_protocol::RemoteFileEntry;

use crate::dedicated_user;
use crate::errors::{AppError, AppResult};
use crate::ssh::SshSession;
// The one shared implementation - every module that builds a remote
// command used to carry its own byte-identical copy of this.
use crate::ssh::command::quote as shell_quote;

use super::sandbox::{relativize, sanitize_relative_path};
use super::{ApplicationFileProvider, ProgressFn};

const HELPER_PATH: &str = "/usr/local/lib/vibessh/file-helper.sh";
const SUDOERS_PATH: &str = "/etc/sudoers.d/vibessh-file-helper";
/// Every staging file lives under this directory - the helper script itself
/// refuses to `read`/`write` a staging argument that doesn't start with it
/// (`require_staging`), a defense-in-depth check against a hypothetical bug
/// on the Rust side ever passing through some other path.
///
/// **This used to be `/tmp/vibessh-stage-`, and that was two separate
/// vulnerabilities.** `/tmp` is world-readable and the helper explicitly
/// `chmod 644`'d each staging file, so every file anyone opened in the
/// Files tab became a world-readable copy - readable by every *other*
/// Application's dedicated account, which is exactly the boundary this
/// provider exists to enforce. And the staging file was created by the
/// dedicated account (through `sudo -u`) while the cleanup was attempted
/// over the connecting admin's own SFTP: `/tmp`'s sticky bit means a
/// non-owner cannot unlink, so that cleanup always failed, its error was
/// discarded, and the world-readable copies accumulated forever.
///
/// Both are fixed by the layout in `staging_dir` plus the `sudo rm` in
/// `discard_staging`, and by the staging files now being mode 0600.
pub(crate) const STAGING_ROOT: &str = "/run/vibessh/stage";

/// Per-Application staging directory, owned by that Application's own
/// dedicated account.
///
/// **Mode 0701 is deliberate, not a typo.** Two mutually-untrusting
/// unprivileged identities have to exchange bytes here: the dedicated
/// account (which owns the Application's files and is what the helper runs
/// as) and the connecting SSH admin (which is the only identity SFTP can
/// authenticate as, so it is the only one that can move binary content).
/// Owner `rwx` lets the helper create and read staging files; other `--x`
/// lets the admin *traverse* to a path it already knows without being able
/// to **list** the directory. Staging file names are v4 UUIDs, so another
/// Application's account - which gets the same `--x` and no read - has no
/// way to enumerate or guess one.
///
/// The admin's own half of each handoff goes through `sudo` (see
/// `claim_staging`/`release_staging`) rather than through a shared group:
/// adding the admin to a per-Application group would not take effect on an
/// already-open SSH session anyway, since group membership is fixed at
/// login.
fn staging_dir(application_id: Uuid) -> String {
    format!("{STAGING_ROOT}/{application_id}")
}

/// POSIX `sh`, not bash - portable across every distro this codebase
/// otherwise assumes (Debian/Ubuntu). Every path/mode argument arrives as
/// its own argv element (`sudo` execs this script directly; neither it nor
/// this script's own body re-parses its arguments through a shell), so a
/// value containing spaces/quotes/newlines can never break out of its own
/// argument - the one class of bug this design specifically avoids,
/// compared to building a string for `sh -c`.
fn helper_script() -> String {
    format!(
        r#"#!/bin/sh
set -eu

# `sudo -u <dedicated user>` doesn't change the working directory by
# default - this process inherits whatever directory the connecting admin's
# own SSH session happened to be in (typically their own home directory),
# which the dedicated account has no permission to even read. GNU `find`
# internally tries to `fchdir` back to that starting directory when it's
# done (a defense against a symlink race, unrelated to any path this script
# actually operates on) and fails loudly ("Failed to restore initial
# working directory") if it can't - `cd /` first sidesteps that entirely,
# since `/` is traversable by every account regardless of what it owns.
cd / || exit 1
umask 077

root_arg="$1"; shift
op="$1"; shift

canon_root=$(realpath -e -- "$root_arg") || {{ echo "vibessh-file-helper: application root does not exist" >&2; exit 3; }}

within_root() {{
    case "$1" in
        "$canon_root") return 0 ;;
        "$canon_root"/*) return 0 ;;
        *) return 1 ;;
    esac
}}

resolve_target() {{
    target="$1"
    if [ -e "$target" ] || [ -L "$target" ]; then
        realpath -- "$target"
    else
        parent=$(dirname -- "$target")
        name=$(basename -- "$target")
        parent_canon=$(realpath -e -- "$parent") || return 1
        printf '%s/%s\n' "$parent_canon" "$name"
    fi
}}

require_within_root() {{
    within_root "$1" || {{ echo "vibessh-file-helper: '$1' is outside the application root" >&2; exit 5; }}
}}

require_staging() {{
    case "$1" in
        {staging_root}/*) return 0 ;;
        *) echo "vibessh-file-helper: staging path rejected" >&2; exit 8 ;;
    esac
}}

case "$op" in
  realpath)
    resolved=$(resolve_target "$1") || {{ echo "vibessh-file-helper: no such path" >&2; exit 4; }}
    require_within_root "$resolved"
    printf '%s\n' "$resolved"
    ;;
  list)
    target=$(resolve_target "$1") || {{ echo "vibessh-file-helper: no such directory" >&2; exit 4; }}
    require_within_root "$target"
    printf '%s\n' "$target"
    find "$target" -mindepth 1 -maxdepth 1 -printf '%f\t%y\t%Y\t%s\t%T@\t%m\n'
    ;;
  stat)
    target=$(resolve_target "$1") || {{ echo "vibessh-file-helper: no such path" >&2; exit 4; }}
    require_within_root "$target"
    printf '%s\n' "$target"
    find "$target" -maxdepth 0 -printf '%f\t%y\t%Y\t%s\t%T@\t%m\n'
    ;;
  read)
    target=$(resolve_target "$1") || {{ echo "vibessh-file-helper: no such path" >&2; exit 4; }}
    require_within_root "$target"
    staging="$2"
    require_staging "$staging"
    cp -- "$target" "$staging"
    chmod 600 "$staging"
    ;;
  write)
    target=$(resolve_target "$1") || {{ echo "vibessh-file-helper: no such directory" >&2; exit 4; }}
    require_within_root "$target"
    staging="$2"
    require_staging "$staging"
    cp -- "$staging" "$target"
    ;;
  mkdir)
    target=$(resolve_target "$1") || {{ echo "vibessh-file-helper: no such parent directory" >&2; exit 4; }}
    require_within_root "$target"
    mkdir -- "$target"
    ;;
  delete)
    target=$(resolve_target "$1") || {{ echo "vibessh-file-helper: no such path" >&2; exit 4; }}
    require_within_root "$target"
    if [ "$target" = "$canon_root" ]; then
        echo "vibessh-file-helper: refusing to delete the application root itself" >&2
        exit 6
    fi
    rm -rf -- "$target"
    ;;
  rename)
    from=$(resolve_target "$1") || {{ echo "vibessh-file-helper: no such source path" >&2; exit 4; }}
    require_within_root "$from"
    to=$(resolve_target "$2") || {{ echo "vibessh-file-helper: no such destination directory" >&2; exit 4; }}
    require_within_root "$to"
    mv -- "$from" "$to"
    ;;
  copy)
    from=$(resolve_target "$1") || {{ echo "vibessh-file-helper: no such source path" >&2; exit 4; }}
    require_within_root "$from"
    to=$(resolve_target "$2") || {{ echo "vibessh-file-helper: no such destination directory" >&2; exit 4; }}
    require_within_root "$to"
    cp -r -- "$from" "$to"
    ;;
  chmod)
    target=$(resolve_target "$1") || {{ echo "vibessh-file-helper: no such path" >&2; exit 4; }}
    require_within_root "$target"
    mode="$2"
    case "$mode" in
        ''|*[!0-7]*) echo "vibessh-file-helper: invalid mode" >&2; exit 7 ;;
    esac
    chmod "$mode" -- "$target"
    ;;
  cleanup)
    require_staging "$1"
    rm -f -- "$1"
    ;;
  *)
    echo "vibessh-file-helper: unknown operation '$op'" >&2
    exit 2
    ;;
esac
"#,
        staging_root = STAGING_ROOT,
    )
}

/// Idempotent by *content*, not just presence: compares the already-deployed
/// script's bytes against what `helper_script()` renders right now, and
/// only reinstalls when they actually differ - so editing `HELPER_SCRIPT`'s
/// template in a later change (a new operation, a bug fix like the `cd /`
/// one this function's own history already needed) reaches every Node the
/// next time it provisions a dedicated-user Application, not just ones that
/// never had the helper installed at all. An earlier version of this
/// function only checked "does a file already exist and is it executable",
/// which is exactly the gap that let a real fix ship in code but never
/// actually reach an already-provisioned Node.
///
/// Deploys the script content through the connecting admin's own SFTP write
/// (which lands it *admin*-owned, at a staging path) and only then moves it
/// into place as `root:root` mode `0755` via `sudo install -D` - the
/// admin's own broad `sudo` is what does the actual privilege-crossing
/// install step, same as every other `sudo`-prefixed command this codebase
/// already runs, but the *result* (a root-owned file the admin can no
/// longer just overwrite by hand) is what makes the installed script
/// trustworthy input for the sudoers rule that lets that same admin invoke
/// it as any dedicated per-Application account.
///
/// **Never leaves a bad sudoers rule live.** A syntax error in
/// `/etc/sudoers.d/*` can make `sudo` itself refuse to parse *any* sudoers
/// file system-wide - `visudo -c -f` validates the freshly written rule
/// before trusting it, and this rolls the file back (`rm`) rather than
/// leaving a broken one in place if validation fails. Re-validated on every
/// reinstall, not just the first one, for the same reason the script
/// content itself is - a sudoers rule this function wrote before it knew to
/// validate it deserves the same self-healing.
pub(crate) async fn ensure_helper_installed(connection: &SshSession) -> AppResult<()> {
    let expected = helper_script();
    let deployed = connection.execute_command(&format!("sudo cat {} 2>/dev/null", shell_quote(HELPER_PATH))).await;
    if matches!(deployed, Ok(ref output) if output.stdout == expected) {
        return Ok(());
    }

    // Not an Application staging path: this is the helper script itself,
    // staged by (and owned by) the connecting admin before `sudo install`
    // moves it into place as root. A relative SFTP path lands in the
    // admin's own home directory, which the admin owns outright - so the
    // `rm -f` below actually succeeds, and no world-writable directory is
    // involved. The content is not secret (it is this binary's own embedded
    // script), so the concern here is only integrity and cleanup.
    let staging = format!(".vibessh-helper-{}", Uuid::new_v4());
    connection.write_file(&staging, expected.as_bytes()).await?;

    let runas_group = format!("%{}", dedicated_user::GROUP);
    let script = format!(
        "sudo groupadd --system {group} 2>/dev/null; \
         sudo install -D -o root -g root -m 0755 {staging} {helper}; \
         rc=$?; rm -f {staging}; [ $rc -eq 0 ] || exit $rc; \
         printf '%s ALL=({runas_group}) NOPASSWD: %s\\n' \"$(whoami)\" {helper} | sudo tee {sudoers} >/dev/null && \
         sudo chmod 440 {sudoers} && \
         sudo visudo -c -f {sudoers} >/dev/null || {{ sudo rm -f {sudoers}; exit 9; }}",
        group = dedicated_user::GROUP,
        staging = shell_quote(&staging),
        helper = shell_quote(HELPER_PATH),
        sudoers = shell_quote(SUDOERS_PATH),
    );
    let output = connection.execute_command(&script).await?;
    if output.exit_code != 0 {
        let detail = output.stderr.trim();
        let detail = if detail.is_empty() { "couldn't install the file-operation helper".to_string() } else { detail.to_string() };
        return Err(AppError::Connection(detail));
    }
    Ok(())
}

fn staging_path(application_id: Uuid) -> String {
    format!("{}/{}", staging_dir(application_id), Uuid::new_v4())
}


/// Parses one `find -printf '%f\t%y\t%Y\t%s\t%T@\t%m'` line: bare name,
/// lstat type char (`is_symlink = 'l'`), dereferenced type char
/// (`is_dir = 'd'` - `%Y` follows a symlink, matching
/// `LocalApplicationFileProvider`'s own "is_dir follows, is_symlink
/// doesn't" convention), size in bytes, mtime as an epoch-seconds float,
/// octal permission bits. `absolute_path` is provided by the caller (either
/// `list`'s own canonical directory + this entry's name, or `stat`'s single
/// already-canonical target) rather than reconstructed here, since a `stat`
/// call already knows the full path and doesn't need it re-derived from a
/// bare basename.
fn parse_entry_line(line: &str, absolute_path: &str, canonical_root: &str) -> AppResult<RemoteFileEntry> {
    let mut fields = line.splitn(6, '\t');
    let name = fields.next().filter(|n| !n.is_empty()).ok_or_else(|| AppError::Connection("malformed listing output from the file helper".into()))?.to_string();
    let lstat_type = fields.next().unwrap_or("");
    let deref_type = fields.next().unwrap_or("");
    let size: u64 = fields.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let modified_at = fields.next().and_then(parse_epoch_seconds);
    let permissions = fields.next().and_then(|s| u32::from_str_radix(s.trim(), 8).ok());
    Ok(RemoteFileEntry {
        name,
        path: relativize(absolute_path, canonical_root),
        is_dir: deref_type == "d",
        is_symlink: lstat_type == "l",
        size,
        modified_at,
        permissions,
    })
}

fn parse_epoch_seconds(value: &str) -> Option<DateTime<Utc>> {
    let seconds: f64 = value.trim().parse().ok()?;
    DateTime::from_timestamp(seconds.trunc() as i64, ((seconds.fract()) * 1_000_000_000.0).round().clamp(0.0, 999_999_999.0) as u32)
}

pub struct SudoUserApplicationFileProvider {
    connection: Arc<SshSession>,
    root: String,
    username: String,
    /// Only used to derive this Application's own staging directory - see
    /// `staging_dir` for why staging is per-Application rather than one
    /// shared location.
    application_id: Uuid,
}

impl SudoUserApplicationFileProvider {
    pub fn new(connection: Arc<SshSession>, root: String, username: String, application_id: Uuid) -> Self {
        Self { connection, root, username, application_id }
    }

    /// Creates this Application's staging directory if it isn't there yet.
    /// Cheap and idempotent (`install -d` re-applies owner and mode), and
    /// called at the start of every staged transfer rather than once at
    /// provisioning time, so a Node upgraded from a build that staged
    /// through `/tmp` heals itself with no manual step.
    async fn ensure_staging_dir(&self) -> AppResult<()> {
        let command = staging_dir_command(&self.username, self.application_id);
        let output = self.connection.execute_command(&command).await?;
        if output.exit_code != 0 {
            let detail = output.stderr.trim();
            let detail = if detail.is_empty() { "couldn't prepare the staging directory".to_string() } else { detail.to_string() };
            return Err(AppError::Connection(detail));
        }
        Ok(())
    }

    /// Hands a staging file the *helper* created (owned by the dedicated
    /// account) over to the connecting admin, so the admin's SFTP session
    /// can read it. The admin's own broad `sudo` does this - the same
    /// privilege it already uses to install the helper and the sudoers
    /// rule. Mode stays 0600 throughout; only the owner changes.
    async fn claim_staging(&self, staging: &str) -> AppResult<()> {
        let command = format!(
            "sudo chown \"$(id -un)\":\"$(id -gn)\" {path} && sudo chmod 600 {path}",
            path = shell_quote(staging),
        );
        let output = self.connection.execute_command(&command).await?;
        if output.exit_code != 0 {
            return Err(AppError::Connection("couldn't stage the file for transfer".into()));
        }
        Ok(())
    }

    /// The reverse handoff: a staging file the admin just uploaded over
    /// SFTP becomes owned by the dedicated account, so the helper (which
    /// runs as that account) can read it back out.
    async fn release_staging(&self, staging: &str) -> AppResult<()> {
        let command = release_staging_command(&self.username, staging);
        let output = self.connection.execute_command(&command).await?;
        if output.exit_code != 0 {
            return Err(AppError::Connection("couldn't stage the file for transfer".into()));
        }
        Ok(())
    }

    /// Creates an empty, admin-owned staging file so the admin's SFTP write
    /// can land in a directory it does not own. `install /dev/null` is what
    /// makes this one step rather than a create-then-chown race.
    async fn reserve_staging(&self, staging: &str) -> AppResult<()> {
        let command = format!("sudo install -o \"$(id -un)\" -g \"$(id -gn)\" -m 600 /dev/null {}", shell_quote(staging));
        let output = self.connection.execute_command(&command).await?;
        if output.exit_code != 0 {
            let detail = output.stderr.trim();
            let detail = if detail.is_empty() { "couldn't prepare the staging file".to_string() } else { detail.to_string() };
            return Err(AppError::Connection(detail));
        }
        Ok(())
    }

    /// Removes a staging file, whoever ended up owning it, and says so in
    /// the log when it cannot.
    ///
    /// Deliberately `sudo rm` rather than the admin's own SFTP `remove`:
    /// the previous implementation used SFTP, which cannot unlink a file
    /// owned by another account in a sticky directory, so **every** staged
    /// read left a permanent copy behind - and because the result was
    /// discarded with `let _`, nothing ever reported it. Root can always
    /// unlink, and a failure here is at least visible now.
    async fn discard_staging(&self, staging: &str) {
        let command = format!("sudo rm -f {}", shell_quote(staging));
        match self.connection.execute_command(&command).await {
            Ok(output) if output.exit_code == 0 => {}
            Ok(output) => log::warn!("couldn't remove the staging file {staging}: {}", output.stderr.trim()),
            Err(err) => log::warn!("couldn't remove the staging file {staging}: {err}"),
        }
    }

    /// Sanitizes and joins `relative` onto `root` - deliberately *not*
    /// canonicalized here (unlike `SftpApplicationFileProvider::resolve`,
    /// which has its own `REALPATH` call available at zero extra cost). The
    /// helper script does its own `realpath`-based resolution and
    /// containment check on every single call regardless (see its own
    /// `resolve_target`/`within_root`) - duplicating that in Rust too would
    /// just be a second implementation of the same logic to keep in sync,
    /// not a real extra safety margin.
    fn resolve(&self, relative: &str) -> AppResult<String> {
        let relative = sanitize_relative_path(relative)?;
        Ok(if relative.is_empty() { self.root.clone() } else { format!("{}/{}", self.root.trim_end_matches('/'), relative) })
    }

    async fn run_helper(&self, op: &str, args: &[&str]) -> AppResult<String> {
        let mut command = format!("sudo -u {} {} {} {}", shell_quote(&self.username), shell_quote(HELPER_PATH), shell_quote(&self.root), shell_quote(op));
        for arg in args {
            command.push(' ');
            command.push_str(&shell_quote(arg));
        }
        let output = self.connection.execute_command(&command).await?;
        if output.exit_code != 0 {
            let detail = output.stderr.trim();
            let detail = if detail.is_empty() { format!("'{op}' failed") } else { detail.to_string() };
            return Err(AppError::Connection(detail));
        }
        Ok(output.stdout)
    }

    async fn canonical_root(&self) -> AppResult<String> {
        Ok(self.run_helper("realpath", &[&self.root]).await?.trim().to_string())
    }
}

#[async_trait::async_trait]
impl ApplicationFileProvider for SudoUserApplicationFileProvider {
    async fn list_directory(&self, path: &str) -> AppResult<Vec<RemoteFileEntry>> {
        let dir = self.resolve(path)?;
        let canonical_root = self.canonical_root().await?;
        let output = self.run_helper("list", &[&dir]).await?;
        let mut lines = output.lines();
        let canonical_dir = lines.next().ok_or_else(|| AppError::Connection("malformed listing output from the file helper".into()))?;
        lines
            .filter(|line| !line.is_empty())
            .map(|line| {
                let name = line.split('\t').next().unwrap_or("");
                let absolute_path = format!("{}/{}", canonical_dir.trim_end_matches('/'), name);
                parse_entry_line(line, &absolute_path, &canonical_root)
            })
            .collect()
    }

    async fn metadata(&self, path: &str) -> AppResult<RemoteFileEntry> {
        let resolved = self.resolve(path)?;
        let canonical_root = self.canonical_root().await?;
        let output = self.run_helper("stat", &[&resolved]).await?;
        let mut lines = output.lines();
        let canonical_path = lines.next().ok_or_else(|| AppError::Connection("malformed stat output from the file helper".into()))?.to_string();
        let stat_line = lines.next().ok_or_else(|| AppError::Connection("malformed stat output from the file helper".into()))?;
        parse_entry_line(stat_line, &canonical_path, &canonical_root)
    }

    async fn read_file(&self, path: &str) -> AppResult<Vec<u8>> {
        let resolved = self.resolve(path)?;
        self.ensure_staging_dir().await?;
        let staging = staging_path(self.application_id);
        self.run_helper("read", &[&resolved, &staging]).await?;
        let result = match self.claim_staging(&staging).await {
            Ok(()) => self.connection.read_file(&staging).await,
            Err(err) => Err(err),
        };
        self.discard_staging(&staging).await;
        result
    }

    async fn write_file(&self, path: &str, contents: &[u8]) -> AppResult<()> {
        let resolved = self.resolve(path)?;
        self.ensure_staging_dir().await?;
        let staging = staging_path(self.application_id);
        let result = async {
            self.reserve_staging(&staging).await?;
            self.connection.write_file(&staging, contents).await?;
            self.release_staging(&staging).await?;
            self.run_helper("write", &[&resolved, &staging]).await.map(|_| ())
        }
        .await;
        self.discard_staging(&staging).await;
        result
    }

    async fn create_directory(&self, path: &str) -> AppResult<()> {
        let resolved = self.resolve(path)?;
        self.run_helper("mkdir", &[&resolved]).await.map(|_| ())
    }

    async fn delete(&self, path: &str) -> AppResult<()> {
        let resolved = self.resolve(path)?;
        self.run_helper("delete", &[&resolved]).await.map(|_| ())
    }

    async fn rename(&self, from: &str, to: &str) -> AppResult<()> {
        let from_resolved = self.resolve(from)?;
        let to_resolved = self.resolve(to)?;
        self.run_helper("rename", &[&from_resolved, &to_resolved]).await.map(|_| ())
    }

    async fn copy(&self, from: &str, to: &str) -> AppResult<()> {
        let from_resolved = self.resolve(from)?;
        let to_resolved = self.resolve(to)?;
        self.run_helper("copy", &[&from_resolved, &to_resolved]).await.map(|_| ())
    }

    async fn set_permissions(&self, path: &str, mode: u32) -> AppResult<()> {
        let resolved = self.resolve(path)?;
        let mode_octal = format!("{mode:o}");
        // `mode` is a u32 straight off the wire, so a caller could send
        // something that renders as more than four octal digits. The helper
        // script rejects it too, but failing here gives a message naming the
        // mode rather than a bare non-zero exit.
        crate::ssh::command::validate_octal_mode(&mode_octal, "the permission mode")?;
        self.run_helper("chmod", &[&resolved, &mode_octal]).await.map(|_| ())
    }

    async fn download_file(&self, path: &str, local_dest: &Path, on_progress: ProgressFn<'_>) -> AppResult<()> {
        let resolved = self.resolve(path)?;
        self.ensure_staging_dir().await?;
        let staging = staging_path(self.application_id);
        self.run_helper("read", &[&resolved, &staging]).await?;
        let result = match self.claim_staging(&staging).await {
            Ok(()) => self.connection.download_file_with_progress(&staging, local_dest, on_progress).await,
            Err(err) => Err(err),
        };
        self.discard_staging(&staging).await;
        result
    }

    async fn upload_file(&self, local_src: &Path, path: &str, on_progress: ProgressFn<'_>) -> AppResult<()> {
        let resolved = self.resolve(path)?;
        self.ensure_staging_dir().await?;
        let staging = staging_path(self.application_id);
        let result = async {
            self.reserve_staging(&staging).await?;
            self.connection.upload_file_with_progress(local_src, &staging, on_progress).await?;
            self.release_staging(&staging).await?;
            self.run_helper("write", &[&resolved, &staging]).await.map(|_| ())
        }
        .await;
        self.discard_staging(&staging).await;
        result
    }
}


/// The command that creates an Application's staging directory.
///
/// A free function so the string a Node actually receives can be asserted
/// on. The bug this shape exists to prevent shipped as `-g {user}`: there is
/// no group named after the account, because `dedicated_user::
/// ensure_provisioned` creates it with `useradd --gid vibessh-apps`. Every
/// staged file operation failed with "invalid group", which is the whole
/// Files tab for any Application running under a dedicated account.
fn staging_dir_command(username: &str, application_id: Uuid) -> String {
    format!(
        "sudo install -d -o root -g root -m 755 {root} && sudo install -d -o {user} -g {group} -m 701 {dir}",
        root = shell_quote(STAGING_ROOT),
        user = shell_quote(username),
        // Mode 701 is what keeps the shared group harmless: the group bit is
        // zero, so the other accounts in `vibessh-apps` gain nothing from
        // being in it.
        group = crate::dedicated_user::GROUP,
        dir = shell_quote(&staging_dir(application_id)),
    )
}

/// The command that hands a staged file over to the dedicated account.
fn release_staging_command(username: &str, staging: &str) -> String {
    format!(
        "sudo chown {user}:{group} {path} && sudo chmod 600 {path}",
        user = shell_quote(username),
        // Same correction, same reason it is safe: mode 600 leaves the group
        // with nothing.
        group = crate::dedicated_user::GROUP,
        path = shell_quote(staging),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_entry_line_reads_every_field_and_relativizes_the_path() {
        let entry = parse_entry_line("velocity.toml\tf\tf\t8912\t1735689600.5\t644", "/srv/app/velocity.toml", "/srv/app").unwrap();
        assert_eq!(entry.name, "velocity.toml");
        assert_eq!(entry.path, "velocity.toml");
        assert!(!entry.is_dir);
        assert!(!entry.is_symlink);
        assert_eq!(entry.size, 8912);
        assert_eq!(entry.permissions, Some(0o644));
        assert_eq!(entry.modified_at.unwrap().timestamp(), 1735689600);
    }

    #[test]
    fn parse_entry_line_treats_a_symlink_to_a_directory_as_a_directory_but_still_flags_the_symlink() {
        let entry = parse_entry_line("plugins-link\tl\td\t0\t0\t777", "/srv/app/plugins-link", "/srv/app").unwrap();
        assert!(entry.is_dir);
        assert!(entry.is_symlink);
    }

    #[test]
    fn parse_entry_line_rejects_an_empty_line() {
        assert!(parse_entry_line("", "/srv/app/x", "/srv/app").is_err());
    }

    #[test]
    fn helper_script_embeds_the_staging_prefix_and_every_operation() {
        let script = helper_script();
        assert!(script.starts_with("#!/bin/sh"));
        assert!(script.contains(STAGING_ROOT));
        for op in ["realpath", "list", "stat", "read", "write", "mkdir", "delete", "rename", "copy", "chmod", "cleanup"] {
            assert!(script.contains(&format!("{op})")), "missing '{op}' case in helper script");
        }
        // No stray unescaped format-brace made it into the generated
        // script - a `{`/`}` left un-doubled in the template would either
        // fail to compile (an unknown/duplicate named argument) or silently
        // vanish from the output.
        assert!(script.contains("within_root() {"));
        assert!(script.contains("esac"));
    }

    /// The regression test for the disclosure half of the staging finding:
    /// the helper used to `chmod 644` every staged file in world-readable
    /// `/tmp`, which made every file anyone opened in the Files tab
    /// readable by every other Application's dedicated account.
    #[test]
    fn helper_script_never_makes_a_staging_file_readable_to_anyone_else() {
        let script = helper_script();
        assert!(!script.contains("chmod 644"), "{script}");
        assert!(script.contains("chmod 600 \"$staging\""), "{script}");
        assert!(script.contains("umask 077"), "{script}");
    }

    /// The regression test for the leak half: nothing may stage through
    /// `/tmp` at all. A sticky world-writable directory is both readable by
    /// everyone and impossible for a non-owner to clean up, which is why
    /// the old staging files accumulated forever.
    #[test]
    fn nothing_stages_through_tmp() {
        assert!(!STAGING_ROOT.starts_with("/tmp"), "{STAGING_ROOT}");
        assert!(!helper_script().contains("/tmp"), "{}", helper_script());
        let staging = staging_path(Uuid::new_v4());
        assert!(staging.starts_with(&format!("{STAGING_ROOT}/")), "{staging}");
    }

    /// The failure this file shipped with: `install -g <username>` against a
    /// group that does not exist, because accounts are created with
    /// `useradd --gid vibessh-apps` and no per-user group is ever made. The
    /// Node answered "install: invalid group" and every staged file
    /// operation died, which is the entire Files tab for a dedicated-user
    /// Application.
    #[test]
    fn staging_is_grouped_by_the_shared_group_not_the_account_name() {
        let id = Uuid::new_v4();
        let username = crate::dedicated_user::username(id);
        let command = staging_dir_command(&username, id);

        assert!(command.contains(&format!("-g {}", crate::dedicated_user::GROUP)), "{command}");
        assert!(!command.contains(&format!("-g '{username}'")), "the account name is not a group: {command}");

        let release = release_staging_command(&username, "/run/vibessh/staging/x");
        assert!(release.contains(&format!("{}:{}", format_args!("'{username}'"), crate::dedicated_user::GROUP)), "{release}");
    }

    /// The shared group is only safe because nothing is readable through
    /// it - every other Application's account is a member.
    #[test]
    fn nothing_is_reachable_through_the_shared_group() {
        let id = Uuid::new_v4();
        let username = crate::dedicated_user::username(id);
        // Directory: owner-only traversal, group and other get no read.
        assert!(staging_dir_command(&username, id).contains("-m 701"));
        // File: owner-only, full stop.
        assert!(release_staging_command(&username, "/run/vibessh/staging/x").contains("chmod 600"));
    }

    /// Staging is per-Application, so one Application's staged bytes never
    /// share a directory with another's.
    #[test]
    fn staging_is_scoped_to_one_application() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        assert_ne!(staging_dir(a), staging_dir(b));
        assert!(staging_path(a).starts_with(&staging_dir(a)));
        assert!(!staging_path(a).starts_with(&staging_dir(b)));
    }

    /// Two different transfers of the same file must not collide, which is
    /// also what makes the directory's `--x`-only mode safe: another
    /// account can traverse but cannot list, so it would have to guess a
    /// v4 UUID to reach anything.
    #[test]
    fn every_staging_path_is_unique() {
        let id = Uuid::new_v4();
        assert_ne!(staging_path(id), staging_path(id));
    }

    #[test]
    fn run_helper_command_shell_quotes_every_argument() {
        // `run_helper`'s command construction is exercised indirectly
        // through `resolve`/`shell_quote`'s own well-tested escaping - this
        // just pins the fixed structure (sudo -u <user> <helper> <root>
        // <op> ...) stays in that order, since a reorder would silently
        // change which argument the helper receives as its root vs op.
        let command = format!("sudo -u {} {} {} {}", shell_quote("vibessh-app-abc123"), shell_quote(HELPER_PATH), shell_quote("/srv/app"), shell_quote("list"));
        assert_eq!(command, "sudo -u 'vibessh-app-abc123' '/usr/local/lib/vibessh/file-helper.sh' '/srv/app' 'list'");
    }
}
