//! Capability detection (Etap I): what this host can actually do, checked
//! fresh on every accepted handshake so it can't go stale across a long
//! agent uptime (Docker or a Minecraft server can start after the agent
//! does). Detects real system state, not whether the agent has a feature
//! *implemented* for it yet - see docs/security/agent-privileges.md in the main
//! repo for why several of these (Docker, systemd unit management) aren't
//! actionable through the agent even when detected as present.

use std::path::Path;

use vibessh_protocol::AgentCapabilities;

pub fn detect() -> AgentCapabilities {
    AgentCapabilities {
        docker: detect_docker(),
        systemd: detect_systemd(),
        minecraft: detect_minecraft(Path::new("/proc")),
        // Baseline: this process can always read/write within its own
        // permissions - unlike Docker/systemd/Minecraft, there's no
        // meaningful "host doesn't have file I/O" case to detect.
        file_access: true,
        terminal: Path::new("/bin/sh").exists(),
    }
}

fn detect_systemd() -> bool {
    Path::new("/run/systemd/system").is_dir()
}

fn detect_docker() -> bool {
    Path::new("/var/run/docker.sock").exists()
}

/// Heuristic: any running process whose command line looks like a
/// Minecraft server - a `java` process launching a recognizable server
/// jar/loader. Not exhaustive (modded/proxy setups vary endlessly) but
/// catches the common vanilla/Paper/Spigot/Purpur/Forge/Fabric case,
/// including Pterodactyl-managed servers, without requiring any
/// configuration from the user. `proc_dir` is parameterized so this can be
/// tested against a fake directory tree instead of the real `/proc`.
fn detect_minecraft(proc_dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(proc_dir) else {
        return false;
    };

    const MARKERS: [&str; 7] = [
        "paper.jar",
        "spigot.jar",
        "purpur.jar",
        "server.jar",
        "fabric-server",
        "forge",
        "bukkit",
    ];

    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().parse::<u32>().is_err() {
            continue; // not a PID directory
        }
        let Ok(cmdline) = std::fs::read(entry.path().join("cmdline")) else {
            continue;
        };
        let cmdline = String::from_utf8_lossy(&cmdline).replace('\0', " ").to_lowercase();
        if cmdline.contains("java") && MARKERS.iter().any(|marker| cmdline.contains(marker)) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_proc_with_process(pid: &str, cmdline: &str) -> tempfile_dir::TempDir {
        let dir = tempfile_dir::TempDir::new();
        let proc_pid = dir.path().join(pid);
        std::fs::create_dir_all(&proc_pid).unwrap();
        std::fs::write(proc_pid.join("cmdline"), cmdline.replace(' ', "\0")).unwrap();
        dir
    }

    #[test]
    fn detects_a_paper_server_process() {
        let dir = fake_proc_with_process("1234", "java -Xmx4G -jar paper.jar nogui");
        assert!(detect_minecraft(dir.path()));
    }

    #[test]
    fn ignores_unrelated_java_processes() {
        let dir = fake_proc_with_process("1234", "java -jar some-web-app.jar");
        assert!(!detect_minecraft(dir.path()));
    }

    #[test]
    fn ignores_non_java_processes_even_with_a_matching_name() {
        let dir = fake_proc_with_process("1234", "cat paper.jar");
        assert!(!detect_minecraft(dir.path()));
    }

    #[test]
    fn ignores_non_pid_directories() {
        let dir = tempfile_dir::TempDir::new();
        std::fs::create_dir_all(dir.path().join("self")).unwrap();
        assert!(!detect_minecraft(dir.path()));
    }

    #[test]
    fn missing_proc_dir_is_not_an_error() {
        let dir = tempfile_dir::TempDir::new();
        assert!(!detect_minecraft(&dir.path().join("does-not-exist")));
    }
}

/// A minimal `TempDir` (create-on-new, remove-on-drop) so this module's
/// tests don't need a real `tempfile` crate dependency for four small
/// fixtures - the whole point of parameterizing `detect_minecraft` was to
/// avoid needing anything beyond the standard library to test it.
#[cfg(test)]
mod tempfile_dir {
    use std::path::{Path, PathBuf};

    pub struct TempDir(PathBuf);

    impl TempDir {
        pub fn new() -> Self {
            let dir = std::env::temp_dir().join(format!("vibessh-agent-test-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        pub fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).ok();
        }
    }
}
