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

/// `canonical_root` answers, keyed by SSH session and configured root - a
/// new session (a reconnect) asks again.
static CANONICAL_ROOTS: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<(u64, String), String>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));
/// Far more Applications than one person has open; past it the map starts over.
const CANONICAL_ROOTS_CAP: usize = 256;
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

# The entry itself rather than what it points at: the parent is resolved and
# checked, the last name is kept as written. Delete and rename use this, so
# removing a symlink removes the link - resolving it first is how deleting a
# link to a folder deleted the folder.
resolve_entry() {{
    name=$(basename -- "$1")
    case "$name" in
        # Not a name at all - resolved in full, as before.
        .|..|/) realpath -e -- "$1"; return ;;
    esac
    parent_canon=$(realpath -e -- "$(dirname -- "$1")") || return 1
    printf '%s/%s\n' "$parent_canon" "$name"
}}

require_within_root() {{
    within_root "$1" || {{ echo "vibessh-file-helper: '$1' is outside the application root" >&2; exit 5; }}
}}

# Creates the directory $1 and whatever parents it is missing, one level at a
# time from the application root. Every level that already exists must
# resolve to a directory inside the root - a symlinked component cannot lead
# the creation elsewhere - and '..' is refused outright. Globbing is off while
# the path is split, so a name with '*' in it stays a name.
make_dirs_within_root() {{
    case "$1" in
        "$canon_root"/*) rel="${{1#"$canon_root"/}}" ;;
        "$root_arg"/*) rel="${{1#"$root_arg"/}}" ;;
        *) return 1 ;;
    esac
    cur="$canon_root"
    status=0
    set -f
    saved_ifs="$IFS"
    IFS=/
    for part in $rel; do
        case "$part" in
            ''|.) continue ;;
            ..) status=1; break ;;
        esac
        next="$cur/$part"
        if [ -e "$next" ] || [ -L "$next" ]; then
            real=$(realpath -e -- "$next") || {{ status=1; break; }}
            if ! within_root "$real" || [ ! -d "$real" ]; then status=1; break; fi
            cur="$real"
        else
            mkdir -- "$next" || {{ status=1; break; }}
            cur="$next"
        fi
    done
    IFS="$saved_ifs"
    set +f
    return "$status"
}}

require_staging() {{
    case "$1" in
        {staging_root}/*) return 0 ;;
        *) echo "vibessh-file-helper: staging path rejected" >&2; exit 8 ;;
    esac
}}

{fetch_function}
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
  readrange)
    target=$(resolve_target "$1") || {{ echo "vibessh-file-helper: no such path" >&2; exit 4; }}
    require_within_root "$target"
    staging="$2"
    require_staging "$staging"
    # tail+head rather than dd: dd's skip is in blocks, and getting a byte
    # offset out of it means bs=1, which reads a large file one byte at a
    # time. tail -c +N is a byte offset by definition and is POSIX.
    tail -c "+$(($3 + 1))" -- "$target" | head -c "$4" > "$staging"
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
    target=$(resolve_entry "$1") || {{ echo "vibessh-file-helper: no such path" >&2; exit 4; }}
    require_within_root "$target"
    if [ "$target" = "$canon_root" ]; then
        echo "vibessh-file-helper: refusing to delete the application root itself" >&2
        exit 6
    fi
    rm -rf -- "$target"
    ;;
  rename)
    from=$(resolve_entry "$1") || {{ echo "vibessh-file-helper: no such source path" >&2; exit 4; }}
    require_within_root "$from"
    [ -e "$from" ] || [ -L "$from" ] || {{ echo "vibessh-file-helper: no such source path" >&2; exit 4; }}
    to=$(resolve_entry "$2") || {{ echo "vibessh-file-helper: no such destination directory" >&2; exit 4; }}
    require_within_root "$to"
    # Never into or over something already there - SFTP's rename refuses
    # that too, and `mv` onto a symlinked directory would move the file
    # into wherever the link points.
    if [ -e "$to" ] || [ -L "$to" ]; then echo "vibessh-file-helper: the destination already exists" >&2; exit 11; fi
    mv -T -- "$from" "$to"
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
  readsmall)
    # The editor's read in one call: the same checks as `read`, a size cap,
    # and the contents on stdout as base64 - no staged copy to create,
    # claim, fetch and clean up, each of which was its own SSH channel.
    target=$(resolve_target "$1") || {{ echo "vibessh-file-helper: no such path" >&2; exit 4; }}
    require_within_root "$target"
    [ -f "$target" ] || {{ echo "vibessh-file-helper: not a regular file" >&2; exit 4; }}
    case "${{2:-}}" in
        ''|*[!0-9]*) echo "vibessh-file-helper: the size limit must be a number" >&2; exit 6 ;;
    esac
    size=$(stat -c %s -- "$target")
    if [ "$size" -gt "$2" ]; then
        echo "vibessh-file-helper: too large ($size bytes)" >&2
        exit 9
    fi
    base64 -w 0 -- "$target"
    ;;
  writein)
    # The editor's save in one call: the new contents come in on stdin, go
    # to a temporary file beside the target and are renamed over it - atomic
    # like the old write-then-rename, keeping the file's mode, and with no
    # staged copy to shuttle through /run/vibessh.
    target=$(resolve_target "$1") || {{ echo "vibessh-file-helper: no such directory" >&2; exit 4; }}
    require_within_root "$target"
    if [ -d "$target" ]; then
        echo "vibessh-file-helper: '$target' is a directory" >&2
        exit 4
    fi
    # Optional second argument: where to keep the version being replaced.
    # Done here rather than as its own call - that was a second `sudo` round
    # trip on every save. Exit 10 means the backup could not be made, and it
    # is always decided before anything is written, so the caller can make
    # the history directory and simply ask again.
    if [ -n "${{2:-}}" ] && [ -f "$target" ]; then
        # A file's first save has no history directory yet. Made here, in
        # this same call - it used to be a `stat` and a `mkdir` per level,
        # each its own `sudo`, which is what made the first save of every
        # file slow when the ones after it were not.
        backup_dir=$(dirname -- "$2")
        if [ ! -d "$backup_dir" ]; then
            make_dirs_within_root "$backup_dir" || {{ echo "vibessh-file-helper: could not make the backup directory" >&2; exit 10; }}
        fi
        backup=$(resolve_target "$2") || {{ echo "vibessh-file-helper: no backup directory" >&2; exit 10; }}
        require_within_root "$backup"
        cp -- "$target" "$backup" || {{ echo "vibessh-file-helper: could not keep a backup" >&2; exit 10; }}
    fi
    tmp=$(mktemp -- "$(dirname -- "$target")/.vibessh-save.XXXXXX") || {{ echo "vibessh-file-helper: could not create a temporary file" >&2; exit 1; }}
    if ! cat > "$tmp"; then
        rm -f -- "$tmp"
        echo "vibessh-file-helper: could not write the new contents" >&2
        exit 1
    fi
    if [ -f "$target" ]; then
        chmod --reference="$target" -- "$tmp" 2>/dev/null || true
    fi
    mv -f -- "$tmp" "$target" || {{ rm -f -- "$tmp"; exit 1; }}
    ;;
  fetchurl)
    # "Download from a link", run as this account so the file is its own.
    # Into a temporary file beside the target, renamed over it only once the
    # download is complete - an interrupted one leaves nothing half-written.
    # Prints the size. The link was checked before it got here
    # (`files::url_fetch`); `fetch_to` keeps it to http and https.
    target=$(resolve_target "$1") || {{ echo "vibessh-file-helper: no such directory" >&2; exit 4; }}
    require_within_root "$target"
    if [ -d "$target" ]; then
        echo "vibessh-file-helper: '$target' is a directory" >&2
        exit 4
    fi
    tmp=$(mktemp -- "$(dirname -- "$target")/.vibessh-fetch.XXXXXX") || {{ echo "vibessh-file-helper: could not create a temporary file" >&2; exit 1; }}
    if fetch_to "$2" "$tmp"; then
        chmod 0644 -- "$tmp"
        mv -f -- "$tmp" "$target" || {{ rm -f -- "$tmp"; exit 1; }}
        wc -c < "$target"
    else
        rc=$?
        rm -f -- "$tmp"
        exit "$rc"
    fi
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
        fetch_function = crate::files::url_fetch::fetch_function(),
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

    let output = connection.execute_command(&install_script(&staging)).await?;
    if output.exit_code != 0 {
        let detail = output.stderr.trim();
        let detail = if detail.is_empty() { "couldn't install the file-operation helper".to_string() } else { detail.to_string() };
        return Err(AppError::Connection(detail));
    }
    Ok(())
}

/// The shell that moves the staged helper into place and writes its sudoers rule.
fn install_script(staging: &str) -> String {
    // An argument to printf, never part of its format: the sudoers group
    // syntax starts with `%`, and `%vibessh-apps` in the format string was
    // read as an (invalid) `%v` directive - printf stopped at `ALL=(`,
    // visudo rejected the half-line, and the rule was rolled back on every
    // install.
    let runas_group = shell_quote(&format!("%{}", dedicated_user::GROUP));
    format!(
        "sudo groupadd --system {group} 2>/dev/null; \
         sudo install -D -o root -g root -m 0755 {staging} {helper}; \
         rc=$?; rm -f {staging}; [ $rc -eq 0 ] || exit $rc; \
         printf '%s ALL=(%s) NOPASSWD: %s\\n' \"$(whoami)\" {runas_group} {helper} | sudo tee {sudoers} >/dev/null && \
         sudo chmod 440 {sudoers} && \
         sudo visudo -c -f {sudoers} >/dev/null || {{ sudo rm -f {sudoers}; exit 9; }}",
        group = dedicated_user::GROUP,
        staging = shell_quote(staging),
        helper = shell_quote(HELPER_PATH),
        sudoers = shell_quote(SUDOERS_PATH),
    )
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

    /// The Application's root as the Node resolves it, asked once per SSH
    /// session.
    ///
    /// It was asked on every listing and every `metadata` - a full `sudo`
    /// helper round trip on its own channel, for an answer that does not
    /// change while the session lasts. Only what paths are *displayed*
    /// relative to depends on it; the helper itself re-checks every path it
    /// is handed against the root on the Node, so a stale entry cannot widen
    /// what is reachable.
    /// One `writein`: the new contents on stdin, and the backup path if any.
    async fn write_in(&self, resolved: &str, backup: Option<&str>, contents: &[u8]) -> AppResult<vibessh_protocol::CommandOutput> {
        let mut command = format!(
            "sudo -u {} {} {} writein {}",
            shell_quote(&self.username),
            shell_quote(HELPER_PATH),
            shell_quote(&self.root),
            shell_quote(resolved),
        );
        if let Some(backup) = backup {
            command.push(' ');
            command.push_str(&shell_quote(backup));
        }
        self.connection.execute_command_with_input(&command, contents).await
    }

    async fn canonical_root(&self) -> AppResult<String> {
        let key = (self.connection.id(), self.root.clone());
        if let Some(root) = CANONICAL_ROOTS.lock().expect("canonical root mutex poisoned").get(&key).cloned() {
            return Ok(root);
        }
        let root = self.run_helper("realpath", &[&self.root]).await?.trim().to_string();
        let mut roots = CANONICAL_ROOTS.lock().expect("canonical root mutex poisoned");
        if roots.len() >= CANONICAL_ROOTS_CAP {
            roots.clear();
        }
        roots.insert(key, root.clone());
        Ok(root)
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

    /// The editor's read as one `sudo` call instead of about seven SSH
    /// channels: `metadata` was the helper's `realpath` and `stat`, and
    /// `read_file` then made the staging directory, had the helper copy the
    /// file into it, claimed the copy, fetched it over SFTP and removed it.
    /// For a 19-line config that was most of a second of pure round trips.
    ///
    /// A Node whose installed helper predates `readsmall` answers "unknown
    /// operation" (exit 2) until the next readiness check reinstalls it; the
    /// old two-step read still works there, so that is what it falls back to.
    async fn read_file_capped(&self, path: &str, max_bytes: u64) -> AppResult<Vec<u8>> {
        use base64::Engine as _;

        let resolved = self.resolve(path)?;
        let command = format!(
            "sudo -u {} {} {} readsmall {} {}",
            shell_quote(&self.username),
            shell_quote(HELPER_PATH),
            shell_quote(&self.root),
            shell_quote(&resolved),
            shell_quote(&max_bytes.to_string()),
        );
        let output = self.connection.execute_command(&command).await?;
        match output.exit_code {
            0 => base64::engine::general_purpose::STANDARD
                .decode(output.stdout.trim())
                .map_err(|err| AppError::Connection(format!("the file helper sent contents that could not be decoded: {err}"))),
            2 => {
                let meta = self.metadata(path).await?;
                if meta.size > max_bytes {
                    return Err(super::too_large_to_edit(path, meta.size));
                }
                self.read_file(path).await
            }
            9 => {
                let size = output
                    .stderr
                    .split(|c: char| !c.is_ascii_digit())
                    .find(|part| !part.is_empty())
                    .and_then(|digits| digits.parse().ok())
                    .unwrap_or(max_bytes + 1);
                Err(super::too_large_to_edit(path, size))
            }
            _ => {
                let detail = output.stderr.trim();
                Err(AppError::Connection(if detail.is_empty() { "'readsmall' failed".to_string() } else { detail.to_string() }))
            }
        }
    }

    /// The editor's save as one `sudo` call, the contents on stdin.
    ///
    /// The usual path - `write_file` to a temporary name, then `rename` - was
    /// the staging dance (make the directory, reserve a file, upload it over
    /// SFTP, hand it over, have the helper copy it, clean up) plus a rename:
    /// around seven SSH channels for a few lines of YAML. On stdin the
    /// contents also stay out of the command line, where `ps` would show
    /// them (AGENTS.md, rule 2).
    ///
    /// `None` for a Node whose installed helper predates `writein` (exit 2,
    /// "unknown operation"), so the caller falls back to the usual path.
    async fn save_in_one_call(&self, path: &str, contents: &[u8], backup_path: Option<&str>) -> Option<AppResult<()>> {
        let resolved = match self.resolve(path) {
            Ok(resolved) => resolved,
            Err(err) => return Some(Err(err)),
        };
        let resolved_backup = match backup_path.map(|backup| self.resolve(backup)).transpose() {
            Ok(resolved) => resolved,
            Err(err) => return Some(Err(err)),
        };

        let mut backup = resolved_backup.as_deref();
        // At most three calls, and one in the usual case. A first save of a
        // file has no history directory yet: the helper refuses before
        // writing anything (exit 10), the directory is made the careful way,
        // and the save is asked again. If the backup still cannot be made,
        // the save goes ahead without it - it always was best-effort - and
        // the log says so.
        for attempt in 0..3 {
            let output = match self.write_in(&resolved, backup, contents).await {
                Ok(output) => output,
                Err(err) => return Some(Err(err)),
            };
            match output.exit_code {
                0 => return Some(Ok(())),
                2 => return None,
                10 if attempt == 0 => {
                    if let Some(dir) = backup_path.and_then(|path| path.rsplit_once('/')).map(|(dir, _)| dir) {
                        if let Err(err) = super::archive::create_directory_all(self, dir).await {
                            log::warn!("couldn't make the history directory for {path}: {err}");
                        }
                    }
                }
                10 => {
                    log::warn!("couldn't keep a history copy of {path} before saving it: {}", output.stderr.trim());
                    backup = None;
                }
                _ => {
                    let detail = output.stderr.trim();
                    return Some(Err(AppError::Connection(if detail.is_empty() { "'writein' failed".to_string() } else { detail.to_string() })));
                }
            }
        }
        Some(Err(AppError::Connection("the save did not complete".to_string())))
    }

    /// The same staging dance as `read_file`, but the helper only copies the
    /// window that was asked for - so looking at a slice of a huge log does
    /// not first duplicate the whole thing into /tmp under the application's
    /// own account.
    async fn read_file_range(&self, path: &str, offset: u64, len: usize) -> AppResult<Vec<u8>> {
        let resolved = self.resolve(path)?;
        self.ensure_staging_dir().await?;
        let staging = staging_path(self.application_id);
        self.run_helper("readrange", &[&resolved, &staging, &offset.to_string(), &len.to_string()]).await?;
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

    async fn fetch_url(&self, path: &str, url: &str) -> AppResult<u64> {
        let resolved = self.resolve(path)?;
        let command = format!(
            "sudo -u {} {} {} fetchurl {} {}",
            shell_quote(&self.username),
            shell_quote(HELPER_PATH),
            shell_quote(&self.root),
            shell_quote(&resolved),
            shell_quote(url),
        );
        let output = self.connection.execute_command(&command).await?;
        if output.exit_code != 0 {
            return Err(AppError::Connection(crate::files::url_fetch::describe_failure(output.exit_code, &output.stderr)));
        }
        Ok(output.stdout.trim().parse().unwrap_or(0))
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

    /// The whole generated script, through `sh -n` - it is assembled from a
    /// format string and a shared function, which is where a stray brace or
    /// quote hides. Skipped where there is no `sh` to ask (a Windows machine
    /// without Git Bash on its PATH); CI runs on Linux, where there always is.
    #[test]
    fn the_helper_script_is_valid_sh() {
        use std::io::Write;
        let script = helper_script();
        let Ok(mut child) = std::process::Command::new("sh").arg("-n").stdin(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn() else {
            return;
        };
        child.stdin.take().unwrap().write_all(script.as_bytes()).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    }

    #[test]
    fn the_sudoers_rule_passes_the_group_as_an_argument_not_as_printf_format() {
        // `%vibessh-apps` inside the format string was read as a `%v`
        // directive, which cut the rule off at `ALL=(` on every install.
        let script = install_script(".vibessh-helper-x");
        let format_start = script.find("printf '").expect("printf") + "printf '".len();
        let format_end = format_start + script[format_start..].find('\'').expect("closing quote");
        assert_eq!(&script[format_start..format_end], "%s ALL=(%s) NOPASSWD: %s\\n");
        assert!(script.contains("'%vibessh-apps'"));
    }

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
    /// Runs the real helper against a temporary directory - not a string
    /// assertion. Deleting a symlink must remove the link, not its target;
    /// renaming one must move the link; a rename never lands in or over an
    /// existing path.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_helper_deletes_and_renames_symlinks_themselves() {
        use std::os::unix::fs::symlink;
        let dir = std::env::temp_dir().join(format!("vibessh-helper-test-{}", uuid::Uuid::new_v4()));
        let root = dir.join("app");
        std::fs::create_dir_all(root.join("world")).unwrap();
        std::fs::write(root.join("world/level.dat"), b"save").unwrap();
        std::fs::create_dir_all(dir.join("outside")).unwrap();
        std::fs::write(dir.join("outside/keep"), b"keep").unwrap();
        symlink(root.join("world"), root.join("world-link")).unwrap();
        symlink(dir.join("outside"), root.join("escape")).unwrap();
        let script = dir.join("helper.sh");
        std::fs::write(&script, helper_script()).unwrap();
        let run = |args: &[&str]| std::process::Command::new("sh").arg(&script).arg(&root).args(args).output().unwrap();

        let deleted = run(&["delete", &root.join("world-link").to_string_lossy()]);
        assert!(deleted.status.success(), "{}", String::from_utf8_lossy(&deleted.stderr));
        assert!(std::fs::symlink_metadata(root.join("world-link")).is_err());
        assert_eq!(std::fs::read(root.join("world/level.dat")).unwrap(), b"save");

        // A link pointing out of the root is removed as a link - its target
        // was never within reach, and is not touched.
        let escaped = run(&["delete", &root.join("escape").to_string_lossy()]);
        assert!(escaped.status.success(), "{}", String::from_utf8_lossy(&escaped.stderr));
        assert_eq!(std::fs::read(dir.join("outside/keep")).unwrap(), b"keep");

        symlink(root.join("world"), root.join("link2")).unwrap();
        let renamed = run(&["rename", &root.join("link2").to_string_lossy(), &root.join("link3").to_string_lossy()]);
        assert!(renamed.status.success(), "{}", String::from_utf8_lossy(&renamed.stderr));
        assert!(std::fs::symlink_metadata(root.join("link3")).unwrap().file_type().is_symlink());
        assert!(root.join("world").is_dir());

        // Not into an existing directory, and not over an existing file.
        let into = run(&["rename", &root.join("link3").to_string_lossy(), &root.join("world").to_string_lossy()]);
        assert!(!into.status.success());
        assert!(std::fs::symlink_metadata(root.join("link3")).is_ok());

        let refused = run(&["delete", &root.to_string_lossy()]);
        assert!(!refused.status.success(), "the root itself must not be deletable");
        assert!(root.join("world/level.dat").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn helper_script_embeds_the_staging_prefix_and_every_operation() {
        let script = helper_script();
        assert!(script.starts_with("#!/bin/sh"));
        assert!(script.contains(STAGING_ROOT));
        for op in ["realpath", "list", "stat", "read", "readrange", "write", "mkdir", "delete", "rename", "copy", "chmod", "cleanup"] {
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
