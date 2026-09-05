use crate::errors::AppResult;
use crate::models::AppInfo;
use crate::state::AppState;

pub fn get_app_info(state: &AppState) -> AppResult<AppInfo> {
    Ok(AppInfo {
        name: state.app_name.clone(),
        version: state.app_version.clone(),
        running_as_root: running_as_root(),
    })
}

/// Whether this process is root.
///
/// **Why it is worth telling somebody.** Nothing in VibeSSH needs local root:
/// it starts processes as the user, reads their files, and everything
/// privileged happens on a Node over SSH. But running the app with `sudo` is
/// a natural thing to try when something does not work, and it breaks the one
/// thing that fails least obviously - root has its own session and cannot see
/// the user's keyring, so storing an SSH password reports
/// "Secret Service: no result found" and reads of stored secrets fail. Two
/// people hit exactly that before this existed.
///
/// Read from `/proc` rather than through libc, which is not a dependency
/// here. A kernel without `/proc/self/status`, or a line this cannot parse,
/// answers "not root": a missing warning is a smaller harm than a false one
/// telling somebody their working setup is wrong.
#[cfg(target_os = "linux")]
fn running_as_root() -> bool {
    let Ok(status) = std::fs::read_to_string("/proc/self/status") else { return false };
    status
        .lines()
        .find_map(|line| line.strip_prefix("Uid:"))
        // "Uid:	real	effective	saved	filesystem" - the effective one
        // is what decides what this process may do.
        .and_then(|ids| ids.split_whitespace().nth(1))
        .map(|effective| effective == "0")
        .unwrap_or(false)
}

#[cfg(not(target_os = "linux"))]
fn running_as_root() -> bool {
    false
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    #[test]
    fn a_normal_test_process_is_not_reported_as_root() {
        // Guards the parse rather than the value: reading the wrong column
        // would make every process look like root, and this is the one
        // assertion that catches it in CI, which does not run as root.
        assert!(!super::running_as_root());
    }
}
