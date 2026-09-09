// Prevents an additional console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    configure_linux_renderer();
    vibessh_lib::run();
}

/// Keeps WebKitGTK's accelerated renderer everywhere it works.
///
/// WebKitGTK 2.42 renders through DMABUF, and on some Linux setups it cannot
/// get an EGL context for it and gives up before the window is drawn:
///
/// ```text
/// Could not create default EGL display: EGL_BAD_PARAMETER. Aborting...
/// ```
///
/// `WEBKIT_DISABLE_DMABUF_RENDERER=1` avoids that, and this used to set it on
/// every Linux machine. It was the wrong trade. Turning the renderer off does
/// not merely lose "some acceleration" - every frame then travels through the
/// CPU, and a person on Fedora with an RX 570 reported an interface running at
/// something like five frames per second while the process itself sat idle.
/// The abort it was avoiding belongs to NVIDIA's own driver stack, so every
/// AMD and Intel machine was paying for a fault it could not have.
///
/// So the workaround now applies where the fault lives. Anyone the detection
/// misses can still ask for it by hand, and the troubleshooting page says so:
///
/// ```sh
/// WEBKIT_DISABLE_DMABUF_RENDERER=1 vibessh
/// ```
#[cfg(target_os = "linux")]
fn configure_linux_renderer() {
    // An explicit choice - either value - is the user's, not ours to revisit.
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_some() {
        return;
    }

    if proprietary_nvidia_driver_loaded(std::path::Path::new("/")) {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }
}

#[cfg(not(target_os = "linux"))]
fn configure_linux_renderer() {}

/// Whether this machine is running NVIDIA's own kernel module.
///
/// Both files exist only when that module is loaded - the proprietary driver
/// and the `nvidia-open` module alike, which share the userspace stack the
/// EGL failure comes from. Nouveau, which goes through Mesa like every other
/// open driver, creates neither and is left on the fast path.
///
/// Reading the filesystem rather than asking a GPU library: this runs before
/// anything is initialised, must not itself be able to fail, and a laptop
/// with the module loaded but rendering on its Intel chip is a machine we
/// would rather slow down than fail to start.
///
/// Takes the root as an argument so the decision can be tested against a
/// directory instead of against whichever machine happens to run the tests.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn proprietary_nvidia_driver_loaded(root: &std::path::Path) -> bool {
    ["proc/driver/nvidia/version", "sys/module/nvidia/version"]
        .iter()
        .any(|relative| root.join(relative).exists())
}

#[cfg(test)]
mod tests {
    use super::proprietary_nvidia_driver_loaded;
    use std::fs;
    use std::path::PathBuf;

    /// A fake `/` with exactly the files named, removed when the test ends.
    struct FakeRoot(PathBuf);

    impl FakeRoot {
        fn with(files: &[&str]) -> Self {
            let path = std::env::temp_dir().join(format!(
                "vibessh-renderer-{}-{}",
                std::process::id(),
                files.join("_").replace(['/', '.'], "-")
            ));
            let _ = fs::remove_dir_all(&path);
            for file in files {
                let full = path.join(file);
                fs::create_dir_all(full.parent().expect("file has a parent")).expect("create fake root");
                fs::write(&full, "test").expect("write fake driver file");
            }
            fs::create_dir_all(&path).expect("create fake root");
            Self(path)
        }
    }

    impl Drop for FakeRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// The machine the reporter was on: AMD, Mesa, no NVIDIA module. It used
    /// to get the workaround anyway, and rendered at a handful of frames a
    /// second because of it.
    #[test]
    fn an_amd_machine_keeps_the_accelerated_renderer() {
        let root = FakeRoot::with(&[]);

        assert!(!proprietary_nvidia_driver_loaded(&root.0));
    }

    #[test]
    fn the_proprietary_driver_is_found_through_proc() {
        let root = FakeRoot::with(&["proc/driver/nvidia/version"]);

        assert!(proprietary_nvidia_driver_loaded(&root.0));
    }

    /// `/proc/driver/nvidia` is absent inside some containers even when the
    /// module is loaded, so the module's own directory is checked too.
    #[test]
    fn the_proprietary_driver_is_found_through_sys() {
        let root = FakeRoot::with(&["sys/module/nvidia/version"]);

        assert!(proprietary_nvidia_driver_loaded(&root.0));
    }

    /// Nouveau is not the driver this works around: it renders through Mesa,
    /// where DMABUF is exactly the path that works. Naming it here so a later
    /// "match anything with nvidia in the name" cannot quietly take the fast
    /// renderer away from it.
    #[test]
    fn nouveau_is_not_the_proprietary_driver() {
        let root = FakeRoot::with(&["sys/module/nouveau/version"]);

        assert!(!proprietary_nvidia_driver_loaded(&root.0));
    }
}
