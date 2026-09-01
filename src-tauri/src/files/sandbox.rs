//! Path sandboxing shared by every `ApplicationFileProvider` - the one
//! place that decides whether a caller-supplied relative path is allowed to
//! touch anything outside an Application's own `working_directory`. A
//! client-side "hide `..`" in the UI is not defense - every provider goes
//! through `sanitize_relative_path` before it ever builds a real filesystem
//! path, and separately checks (see each provider's own `resolve`) that
//! what that path *canonicalizes to* - following any symlinks along the way
//! - still lands inside the root. Two layers, not one: the first stops an
//! obviously malicious path string; the second stops a symlink someone
//! planted earlier from quietly pointing the first layer's "safe" path
//! somewhere it isn't.

use crate::errors::{AppError, AppResult};

/// Turns a caller-supplied path (relative to the application root, `/`- or
/// `\`-separated, e.g. `"plugins/MyPlugin.jar"` or `"."`/`""` for the root
/// itself) into a normalized, traversal-free relative path with no leading
/// slash, no `.` components, and no `..` components - or rejects it
/// outright. A leading `/` (an absolute-looking path) is not an escape by
/// itself here - it just collapses to the same relative path a leading `/`
/// would produce on any join, still confined to the root once joined - but
/// `..` is rejected unconditionally since there's no safe interpretation of
/// it that isn't "go above the root."
pub fn sanitize_relative_path(path: &str) -> AppResult<String> {
    let mut segments: Vec<&str> = Vec::new();
    for segment in path.split(['/', '\\']) {
        match segment {
            "" | "." => continue,
            ".." => return Err(AppError::InvalidInput("path can't contain '..'".into())),
            segment if segment.contains('\0') => return Err(AppError::InvalidInput("path contains a null byte".into())),
            segment => segments.push(segment),
        }
    }
    Ok(segments.join("/"))
}

/// `true` once `candidate` (an absolute, already-canonicalized path or
/// SFTP-server-resolved `REALPATH` string) is `root` itself or genuinely
/// nested under it - a plain string-prefix check would wrongly accept
/// `/srv/app-other` as "under" `/srv/app`, so this always checks for the
/// `/` boundary too.
pub fn is_within_root(candidate: &str, root: &str) -> bool {
    let root = root.trim_end_matches('/');
    candidate == root || candidate.starts_with(&format!("{root}/"))
}

/// The inverse of joining onto `root`: turns an absolute, already-canonical
/// `path` (nested under `root`, or `root` itself) back into the
/// root-relative form `sanitize_relative_path`/each provider's `resolve`
/// expect on the way *in*. Every `ApplicationFileProvider::list_directory`/
/// `metadata` result must go through this before reaching the frontend -
/// otherwise a listed entry's own `.path` (the real absolute host path) fed
/// straight back into `delete`/`rename`/`download`/etc gets joined onto
/// `root` a *second* time, producing a nonexistent nested path (this was a
/// real, previously-shipped bug: deleting a file, or downloading/renaming
/// one, failed with a misleading "the containing directory doesn't exist" /
/// "path escapes the application directory" for anything reached through a
/// real directory listing rather than a blueprint's hardcoded Quick Files
/// path). Falls back to returning `path` unchanged if it isn't actually
/// under `root` - defensive only, every real caller's `path` always is.
pub fn relativize(path: &str, root: &str) -> String {
    let root = root.trim_end_matches('/');
    if path == root {
        return ".".to_string();
    }
    match path.strip_prefix(root).and_then(|rest| rest.strip_prefix('/')) {
        Some(relative) => relative.to_string(),
        None => path.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_accepts_a_plain_relative_path() {
        assert_eq!(sanitize_relative_path("plugins/MyPlugin.jar").unwrap(), "plugins/MyPlugin.jar");
    }

    #[test]
    fn sanitize_treats_root_dot_and_empty_the_same_way() {
        assert_eq!(sanitize_relative_path("").unwrap(), "");
        assert_eq!(sanitize_relative_path(".").unwrap(), "");
        assert_eq!(sanitize_relative_path("./plugins").unwrap(), "plugins");
    }

    #[test]
    fn sanitize_collapses_a_leading_slash_rather_than_treating_it_as_an_escape() {
        assert_eq!(sanitize_relative_path("/plugins/MyPlugin.jar").unwrap(), "plugins/MyPlugin.jar");
    }

    #[test]
    fn sanitize_normalizes_backslashes_the_same_as_forward_slashes() {
        assert_eq!(sanitize_relative_path(r"plugins\MyPlugin.jar").unwrap(), "plugins/MyPlugin.jar");
    }

    #[test]
    fn sanitize_rejects_any_dotdot_component_anywhere_in_the_path() {
        assert!(sanitize_relative_path("..").is_err());
        assert!(sanitize_relative_path("../etc/passwd").is_err());
        assert!(sanitize_relative_path("plugins/../../etc/passwd").is_err());
        assert!(sanitize_relative_path("plugins/..").is_err());
    }

    #[test]
    fn sanitize_rejects_a_null_byte() {
        assert!(sanitize_relative_path("plugins/evil\0.jar").is_err());
    }

    #[test]
    fn is_within_root_accepts_the_root_itself_and_real_children_only() {
        assert!(is_within_root("/srv/app", "/srv/app"));
        assert!(is_within_root("/srv/app/plugins", "/srv/app"));
        assert!(!is_within_root("/srv/app-other", "/srv/app"));
        assert!(!is_within_root("/srv", "/srv/app"));
        assert!(!is_within_root("/etc/passwd", "/srv/app"));
    }

    #[test]
    fn is_within_root_tolerates_a_trailing_slash_on_root() {
        assert!(is_within_root("/srv/app/plugins", "/srv/app/"));
    }

    #[test]
    fn relativize_strips_the_root_prefix_from_a_nested_child() {
        assert_eq!(relativize("/srv/app/plugins/MyPlugin.jar", "/srv/app"), "plugins/MyPlugin.jar");
    }

    #[test]
    fn relativize_maps_root_itself_to_a_dot() {
        assert_eq!(relativize("/srv/app", "/srv/app"), ".");
    }

    #[test]
    fn relativize_round_trips_through_sanitize_and_a_root_join() {
        let root = "/srv/app";
        let absolute = "/srv/app/plugins/MyPlugin.jar";
        let relative = relativize(absolute, root);
        let rejoined = format!("{root}/{}", sanitize_relative_path(&relative).unwrap());
        assert_eq!(rejoined, absolute);
    }
}
