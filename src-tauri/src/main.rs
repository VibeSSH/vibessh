// Prevents an additional console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    disable_webkit_dmabuf_on_linux();
    vibessh_lib::run();
}

/// Stops WebKitGTK aborting before the window is drawn.
///
/// WebKitGTK 2.42 renders through DMABUF, and on a good number of Linux
/// setups it cannot get an EGL context for it and gives up:
///
/// ```text
/// Could not create default EGL display: EGL_BAD_PARAMETER. Aborting...
/// ```
///
/// The window appears for a moment and the process then dies, which reads as
/// the application being broken rather than as a renderer that could have
/// fallen back. Two of the first people to run the Linux build hit it, so
/// this is the common case rather than an exotic one.
///
/// It costs the accelerated compositing path, which is a real loss - but an
/// application that starts everywhere beats a faster one that does not
/// start. Only set when the variable is absent, so anyone who wants the
/// faster path back can ask for it:
///
/// ```sh
/// WEBKIT_DISABLE_DMABUF_RENDERER=0 vibessh
/// ```
#[cfg(target_os = "linux")]
fn disable_webkit_dmabuf_on_linux() {
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }
}

#[cfg(not(target_os = "linux"))]
fn disable_webkit_dmabuf_on_linux() {}
