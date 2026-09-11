use serde::{Deserialize, Serialize};

/// A directory that looks like a game server somebody already set up.
///
/// **What this is for.** Files arrive on a machine long before VibeSSH hears
/// about them - a Pterodactyl install left behind, a server somebody has been
/// running by hand, a folder restored from a backup. Adopting one meant
/// walking the five-step wizard and retyping what is already written in the
/// directory: its name, its jar, its port. Four servers meant doing that four
/// times.
///
/// Everything here is read out of the files rather than asked for.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredServer {
    /// The directory's own name, which is what the server is called for
    /// everyone already using it.
    pub name: String,
    /// Its full path, ready to be an Application's working directory.
    pub path: String,
    /// The jar to run, relative to `path`.
    pub jar: String,
    /// From `server.properties`, when there is one. `None` is normal - a
    /// proxy keeps its port elsewhere - and the wizard asks rather than
    /// guessing.
    pub port: Option<u16>,
    pub kind: DiscoveredServerKind,
}

/// What the jar's name says this is.
///
/// Used to label the row and nothing else. Adoption creates a plain Docker
/// Application whatever this says: taking over version management would mean
/// downloading a different jar into a directory somebody is already running,
/// and that is a decision for them rather than a default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DiscoveredServerKind {
    Paper,
    Purpur,
    Velocity,
    Waterfall,
    Spigot,
    Fabric,
    Forge,
    /// A jar whose name says nothing familiar - still adoptable, just
    /// unlabelled.
    Unknown,
}

impl DiscoveredServerKind {
    /// Read from the filename because that is all there is to read without
    /// opening the archive, and server jars are named after what they are.
    pub fn from_jar_name(jar: &str) -> Self {
        let name = jar.to_lowercase();
        if name.contains("paper") {
            Self::Paper
        } else if name.contains("purpur") {
            Self::Purpur
        } else if name.contains("velocity") {
            Self::Velocity
        } else if name.contains("waterfall") {
            Self::Waterfall
        } else if name.contains("spigot") || name.contains("craftbukkit") {
            Self::Spigot
        } else if name.contains("fabric") {
            Self::Fabric
        } else if name.contains("forge") {
            Self::Forge
        } else {
            Self::Unknown
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_jar_is_recognised_by_what_it_is_called() {
        assert_eq!(DiscoveredServerKind::from_jar_name("paper-1.21.4-232.jar"), DiscoveredServerKind::Paper);
        assert_eq!(DiscoveredServerKind::from_jar_name("velocity-3.4.0-566.jar"), DiscoveredServerKind::Velocity);
        // Case and decoration around the name do not matter.
        assert_eq!(DiscoveredServerKind::from_jar_name("Purpur-1.21.4 (1).jar"), DiscoveredServerKind::Purpur);
    }

    #[test]
    fn an_unfamiliar_jar_is_unknown_rather_than_guessed_at() {
        // Still adoptable - the label is the only thing this decides.
        assert_eq!(DiscoveredServerKind::from_jar_name("server.jar"), DiscoveredServerKind::Unknown);
        assert_eq!(DiscoveredServerKind::from_jar_name("my-custom-build.jar"), DiscoveredServerKind::Unknown);
    }
}
