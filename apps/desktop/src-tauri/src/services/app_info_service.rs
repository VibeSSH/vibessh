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

/// Whether this machine can run containers at all.
///
/// **Asked before anything is created, not after.** Picking the Docker
/// runtime for a Local application used to succeed all the way through the
/// wizard and fail on the first start, with an error about PATH - by which
/// point there is an Application sitting there that cannot run. This is what
/// lets the wizard say so while the answer still costs nothing.
///
/// `docker version` rather than `--version`: the second one only proves the
/// client binary exists, and a Docker Desktop that is installed but not
/// started answers it happily while every real command fails. This talks to
/// the daemon, which is the thing that has to be there.
pub async fn local_docker_available() -> bool {
    let mut command = tokio::process::Command::new("docker");
    command.arg("version");

    #[cfg(windows)]
    {
        // The same reason every other docker invocation carries it: a
        // packaged build is a GUI process, and without this each probe
        // flashes a console window.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    matches!(command.output().await, Ok(output) if output.status.success())
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
