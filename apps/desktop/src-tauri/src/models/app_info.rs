use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub name: String,
    pub version: String,
    /// True when this is running as root on Linux, which quietly breaks
    /// every secret the app stores - see `app_info_service::running_as_root`.
    /// Always false elsewhere: on Windows and macOS an elevated process is
    /// unusual rather than a thing people reach for to "make it work".
    pub running_as_root: bool,
}
