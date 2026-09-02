//! Diagnostic playbooks, matched against the evidence rather than the topic.
//!
//! **Why this exists.** The first real Diagnose turn against a failed Paper
//! server produced a long, hedged answer listing everything that can stop a
//! Minecraft server. The actual cause was a corrupted world, and it was
//! sitting in the log the model had been handed. A general-purpose model
//! reading raw logs knows *of* every cause and has no ranking between them;
//! what it lacks is the operator's habit of recognising one line and
//! stopping.
//!
//! So a skill is not a topic summary. It is a signature plus what that
//! signature means: the log line that identifies a cause, the one-line
//! explanation, and the fix. When a signature matches, the model is told to
//! lead with it instead of enumerating alternatives.
//!
//! **Matching is on evidence first.** A trigger found in the collected
//! context - which is where the logs are - outranks the same trigger found
//! in the user's question, because a log line is a fact and a question is a
//! guess. That ordering is what stops "my server crashed" from pulling in
//! every crash playbook at once.
//!
//! **These are deliberately short.** Each one is a few lines. They ride in
//! the same prompt as the collected snapshot, and a playbook long enough to
//! crowd out the actual logs would recreate the problem it was written to
//! fix.

/// One diagnostic playbook.
pub struct Skill {
    pub id: &'static str,
    /// Blueprint ids this is about. Empty means it applies to anything -
    /// a full disk stops a Minecraft server and a database equally.
    pub blueprints: &'static [&'static str],
    /// Lowercase substrings that identify this cause. Matched against the
    /// collected context and the user's question.
    pub triggers: &'static [&'static str],
    pub title: &'static str,
    pub guidance: &'static str,
}

/// The blueprint ids that are Minecraft servers, as opposed to proxies.
/// Kept as one list because every server-side playbook applies to all of
/// them - Purpur is a fork of Paper, and Paper of Spigot.
const MINECRAFT_SERVERS: &[&str] = &["paper", "purpur"];

/// The proxies. Their failure modes are genuinely different: a proxy has no
/// world and cannot corrupt one, and its characteristic failure is a
/// handshake with a backend rather than anything local.
const MINECRAFT_PROXIES: &[&str] = &["velocity", "waterfall"];

pub const SKILLS: &[Skill] = &[
    Skill {
        id: "mc-world-corrupt",
        blueprints: MINECRAFT_SERVERS,
        triggers: &[
            "level.dat",
            "failed to load level",
            "exception reading",
            "chunk file at",
            "failed to check session lock",
            "anvilchunkloader",
            "region file",
            "corrupt",
        ],
        title: "The world is damaged and the server stops while loading it",
        guidance: "\
The server refuses to start because the world it is told to load will not \
read. This is the most common cause of a Paper/Purpur server that fails \
immediately after startup begins, and the log says so directly.

Take a backup first, from the Backups tab, so anything below is reversible.

Which of the three it is:
- **`level.dat` unreadable.** In the Files tab, open `world/`, delete \
  `level.dat` and rename `level.dat_old` to `level.dat`. If the log also \
  says `No key dimensions in MapLike`, use that playbook instead - it is the \
  same fix with more certainty about the cause.
- **One damaged region.** The log names a chunk or a \
  `world/region/r.X.Z.mca` file. Delete that one file in the Files tab; it \
  loses those chunks and nothing else, and they regenerate when someone \
  walks there.
- **`Failed to check session lock`.** The world is not damaged at all. A \
  second process still holds it, usually a container that was never \
  stopped. Look for a duplicate Application pointed at the same working \
  directory rather than touching the world.",
    },
    Skill {
        id: "mc-leveldat-empty",
        blueprints: MINECRAFT_SERVERS,
        triggers: &[
            "failed to load datapacks",
            "can't proceed with server load",
            "no key dimensions in maplike",
            "no key seed in maplike",
            "no key generator in maplike",
        ],
        title: "level.dat reads as empty, so the server gives up before loading the world",
        guidance: "\
This pair of lines is more specific than it looks, and it is worth not \
mistaking it for a datapack problem. `No key dimensions in MapLike[{}]` \
means the world settings parsed to an *empty* compound: the server read \
`world/level.dat` and got nothing out of it. `Failed to load datapacks` is \
the consequence, not the cause - the datapack list lives inside level.dat, \
so an unreadable level.dat fails there first. A genuinely broken datapack \
names itself in the log.

The file is truncated or zero length, usually from a crash or a full disk \
while it was being written.

In VibeSSH:
1. Backups tab - take a backup first, so this is reversible.
2. Files tab - open `world/`. The server keeps the previous copy as \
   `level.dat_old`. Delete `level.dat`, then rename `level.dat_old` to \
   `level.dat`.
3. Start the Application again.

If `level.dat_old` is missing or fails the same way, the world settings are \
gone. Restoring from Backups is the only way to keep the world; otherwise \
delete the `world` directory in the Files tab and the server generates a new \
one on next start - which loses everything built in it, so confirm the user \
wants that before suggesting it.",
    },
    Skill {
        id: "mc-eula",
        blueprints: MINECRAFT_SERVERS,
        triggers: &["you need to agree", "eula.txt", "eula=false", "failed to load eula"],
        title: "The Minecraft EULA has not been accepted",
        guidance: "\
The server wrote `eula.txt` and stopped. Nothing is broken. In VibeSSH this \
is the blueprint's own EULA checkbox on the Application's configuration - \
tick it and recreate, rather than editing `eula.txt` by hand, so the setting \
survives a recreate.",
    },
    Skill {
        id: "port-bind",
        blueprints: &[],
        triggers: &[
            "failed to bind to port",
            "address already in use",
            "perhaps a server is already running",
            "bind: address already in use",
            "port is already allocated",
        ],
        title: "Something already holds the port",
        guidance: "\
Another process has the port. Two cases, and they are fixed differently:
- Another VibeSSH Application publishes the same external port. The Ports tab \
  will refuse this on creation, but an Application created before that check, \
  or one whose container outlived its row, can still hold it.
- A container from a previous run was never removed. It keeps the port under \
  `--restart unless-stopped` even while the Application reads as stopped.

Change the external port in the Ports tab, or remove whatever holds it. Do \
not change the internal port - that is the port inside the container and \
almost never the problem.",
    },
    Skill {
        id: "java-version",
        blueprints: &["paper", "purpur", "velocity", "waterfall"],
        triggers: &[
            "unsupportedclassversionerror",
            "has been compiled by a more recent version",
            "class file version",
            "requires java",
            "unsupported java",
        ],
        title: "The Java version is too old for this server jar",
        guidance: "\
The jar was built for a newer Java than the container runs. This is a \
one-field fix: raise the Java version on the Application's configuration \
(the blueprint pulls the matching `eclipse-temurin` image) and recreate. \
Recent Minecraft versions need Java 21; older ones run on 17. Nothing needs \
to be installed on the Node - the Java version is part of the image.",
    },
    Skill {
        id: "out-of-memory",
        blueprints: &[],
        triggers: &["outofmemoryerror", "out of memory", "gc overhead limit", "killed process", "oom-killed", "oomkilled", "exit code 137"],
        title: "The process was killed for using too much memory",
        guidance: "\
Exit code 137 or an OOM kill means the kernel or Docker stopped it, not the \
application. Check the Application's memory limit against what the process \
was told it could use - for a Java server, `-Xmx` above the container's \
memory limit is the classic mistake: the JVM believes it may grow, the \
container does not, and Docker wins. Keep `-Xmx` roughly 1GB below the limit \
so the JVM's own non-heap memory fits.

`java.lang.OutOfMemoryError` in the log without an exit code 137 is the \
opposite case: the JVM hit its own `-Xmx` ceiling and that ceiling is too \
low.",
    },
    Skill {
        id: "mc-plugin-failure",
        blueprints: MINECRAFT_SERVERS,
        triggers: &[
            "could not load 'plugins",
            "error occurred while enabling",
            "noclassdeffounderror",
            "invalid plugin.yml",
            "unsupported api version",
        ],
        title: "A plugin is stopping the server from starting",
        guidance: "\
The log names the plugin and the file. Usually it is built for a different \
Minecraft version than the server is running, or a dependency it needs is \
missing. Move that one jar out of `plugins/` in the Files tab and start \
again - if the server comes up, the plugin is confirmed and needs a build \
matching this server version. `Unsupported API version` says this outright.",
    },
    Skill {
        id: "mc-proxy-backend",
        blueprints: MINECRAFT_PROXIES,
        triggers: &[
            "unable to connect to",
            "backend server",
            "modern forwarding",
            "forwarding secret",
            "if you wish to use ip forwarding",
            "server is offline",
        ],
        title: "The proxy cannot reach or authenticate with a backend server",
        guidance: "\
A proxy that starts but cannot pass players through is nearly always one of \
two things:
- **Not reachable.** Since Applications are network-isolated from each other \
  by default, the proxy can only reach a backend that has been explicitly \
  connected to it. Check the connection between the two Applications, then \
  that the proxy's config names the backend by its container name, not \
  `localhost` - `localhost` inside a container is that container.
- **Forwarding mismatch.** Velocity's modern forwarding needs the same \
  secret on both sides and `online-mode=false` on the backend. The backend \
  logs this as a failed handshake, not as a connection error.",
    },
    Skill {
        id: "container-restart-loop",
        blueprints: &[],
        triggers: &["restarting", "back-off restarting", "exited with code", "container is unhealthy", "exit code 1"],
        title: "The container starts and exits immediately, over and over",
        guidance: "\
A container that exits at once and is restarted by its policy produces a \
short log that repeats. Read the *first* few lines of one cycle rather than \
the tail - the tail is the restart, the cause is at the top. If every cycle \
is identical, the cause is configuration rather than anything transient, and \
nothing about restarting it again will change it.",
    },
    Skill {
        id: "db-access-denied",
        blueprints: &["mariadb", "phpmyadmin"],
        triggers: &["access denied for user", "authentication plugin", "password: no", "er_access_denied"],
        title: "The database rejected the credentials",
        guidance: "\
`Access denied for user 'x'@'host'` names both the user and the host it was \
seen from, and the host half is the part people miss: a grant for \
`'user'@'localhost'` does not match a connection arriving from a container's \
address. `(using password: NO)` means no password was sent at all - usually \
an environment variable that is not set on the client side.

For a first start, MariaDB only applies `MYSQL_ROOT_PASSWORD` when its data \
directory is empty. On an existing volume it is ignored, and the old \
password still applies.",
    },
    Skill {
        id: "redis-noauth",
        blueprints: &["redis"],
        triggers: &["noauth authentication required", "wrongpass", "invalid password"],
        title: "Redis wants a password the client is not sending",
        guidance: "\
`NOAUTH Authentication required` means the Application has a password set in \
its blueprint configuration and the client is connecting without one. \
`WRONGPASS` means it sent one that does not match. Either set the client's \
password, or clear the field on the Application - but only if the port is \
not reachable beyond this Node.",
    },
    Skill {
        id: "disk-full",
        blueprints: &[],
        triggers: &["no space left on device", "disk quota exceeded", "write error", "enospc"],
        title: "The Node is out of disk space",
        guidance: "\
Nothing here is the Application's fault and no configuration change fixes \
it. The Node's disk usage is in this snapshot if it could be read. Common \
consumers on a Node running VibeSSH: old container images (`docker image \
prune`), an Application's own logs, and accumulated backups in \
`.vibessh-backups/`.",
    },
    Skill {
        id: "docker-unavailable",
        blueprints: &[],
        triggers: &["cannot connect to the docker daemon", "docker: command not found", "is the docker daemon running", "permission denied while trying to connect to the docker"],
        title: "Docker itself is not answering on the Node",
        guidance: "\
No Application on this Node will start until this is fixed, so it is worth \
confirming before looking at anything else. Either the daemon is not running \
(`systemctl status docker`) or the account VibeSSH connects as cannot reach \
its socket. VibeSSH can install Docker from the Node's own page.",
    },
];

/// How strongly a trigger found in the collected context counts.
///
/// Higher than the question, because a log line is evidence and a question
/// is a guess. Somebody who says "it crashed" should not pull in every
/// crash playbook while the log is sitting there naming one.
const CONTEXT_WEIGHT: usize = 4;
const QUESTION_WEIGHT: usize = 2;
const BLUEPRINT_WEIGHT: usize = 3;

/// The playbooks worth attaching to one turn, best first.
///
/// A blueprint match alone is not enough to attach anything: knowing the
/// Application is a Paper server does not make the world-corruption playbook
/// relevant, and attaching all of them would be the enumeration this module
/// exists to avoid. At least one trigger has to actually appear.
pub fn match_skills(context: Option<&str>, question: &str, blueprint: Option<&str>, limit: usize) -> Vec<&'static Skill> {
    if limit == 0 {
        return Vec::new();
    }
    let context = context.map(str::to_lowercase);
    let question = question.to_lowercase();

    let mut scored: Vec<(usize, &'static Skill)> = SKILLS
        .iter()
        .filter_map(|skill| {
            if !skill.blueprints.is_empty() {
                // A playbook scoped to blueprints must not fire for an
                // unrelated one - Redis auth advice on a Minecraft server is
                // worse than no advice.
                match blueprint {
                    Some(id) if skill.blueprints.contains(&id) => {}
                    _ => return None,
                }
            }

            let mut score = 0;
            for trigger in skill.triggers {
                if context.as_deref().is_some_and(|text| text.contains(trigger)) {
                    score += CONTEXT_WEIGHT;
                }
                if question.contains(trigger) {
                    score += QUESTION_WEIGHT;
                }
            }
            if score == 0 {
                return None;
            }
            if blueprint.is_some_and(|id| skill.blueprints.contains(&id)) {
                score += BLUEPRINT_WEIGHT;
            }
            Some((score, skill))
        })
        .collect();

    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.id.cmp(b.1.id)));
    scored.into_iter().take(limit).map(|(_, skill)| skill).collect()
}

impl Skill {
    pub fn to_prompt_text(&self) -> String {
        format!("[{}]\n{}", self.title, self.guidance)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(skills: &[&Skill]) -> Vec<&'static str> {
        skills.iter().map(|s| s.id).collect()
    }

    /// The case this module was written for: the answer was long and hedged
    /// when the log said plainly what had happened.
    #[test]
    fn a_corrupt_world_in_the_log_is_the_first_playbook_offered() {
        let context = "Recent log lines\n[12:00:01] [Server thread/ERROR]: Failed to load level.dat\njava.io.IOException: Exception reading world/level.dat";
        let matched = match_skills(Some(context), "why is it down?", Some("paper"), 3);
        assert_eq!(matched.first().map(|s| s.id), Some("mc-world-corrupt"));
    }

    /// The exact pair of lines off a real failed server. The model read them
    /// as "a damaged datapack or maybe the world data" and offered both. They
    /// are more specific than that: the datapack list lives inside level.dat,
    /// so an unreadable level.dat fails there first, and `No key dimensions
    /// in MapLike[{}]` says the file parsed to nothing.
    #[test]
    fn the_datapack_message_resolves_to_the_level_dat_playbook() {
        let context = "[12:00:01] [Server thread/ERROR]: Failed to load datapacks, can't proceed with server load
                       No key dimensions in MapLike[{}]; No key seed in MapLike[{}]";
        let matched = match_skills(Some(context), "nie startuje", Some("paper"), 2);
        assert_eq!(matched.first().map(|s| s.id), Some("mc-leveldat-empty"));
    }

    /// Every fix in a playbook has to be something the interface can
    /// actually do, because the assistant is told to give steps rather than
    /// commands. A playbook full of shell would quietly undo that.
    #[test]
    fn playbooks_do_not_hand_out_shell_commands() {
        for skill in SKILLS {
            for forbidden in ["$ ", "sudo ", "rm -rf", "cp -a", "mv /", "docker run"] {
                assert!(!skill.guidance.contains(forbidden), "{} contains a shell command: {forbidden:?}", skill.id);
            }
        }
    }

    /// Evidence beats topic. The question mentions memory, the log names the
    /// world - the log wins.
    #[test]
    fn evidence_in_the_context_outranks_a_guess_in_the_question() {
        let context = "Failed to load level.dat";
        let matched = match_skills(Some(context), "is it out of memory?", Some("paper"), 3);
        assert_eq!(matched.first().map(|s| s.id), Some("mc-world-corrupt"));
        // The guess is still offered, just second - the user asked.
        assert!(ids(&matched).contains(&"out-of-memory"));
    }

    /// Knowing it is a Paper server is not a reason to attach Paper
    /// playbooks. Without a signature there is nothing to lead with, and
    /// attaching all of them is the enumeration this exists to prevent.
    #[test]
    fn a_blueprint_alone_attaches_nothing() {
        let matched = match_skills(Some("Application\nName: survival\nStored status: running"), "how do I add a plugin?", Some("paper"), 3);
        assert!(matched.is_empty(), "expected no playbooks, got {:?}", ids(&matched));
    }

    #[test]
    fn a_playbook_scoped_to_one_blueprint_never_fires_for_another() {
        // The Redis wording appears verbatim, but this is a Paper server.
        let matched = match_skills(Some("NOAUTH Authentication required"), "what is this?", Some("paper"), 3);
        assert!(!ids(&matched).contains(&"redis-noauth"));

        let matched = match_skills(Some("NOAUTH Authentication required"), "what is this?", Some("redis"), 3);
        assert_eq!(ids(&matched), vec!["redis-noauth"]);
    }

    /// A full disk or a taken port is not anybody's blueprint's business.
    #[test]
    fn unscoped_playbooks_fire_whatever_the_application_is() {
        for blueprint in [Some("paper"), Some("mariadb"), Some("nats"), None] {
            let matched = match_skills(Some("write failed: No space left on device"), "", blueprint, 3);
            assert!(ids(&matched).contains(&"disk-full"), "disk-full missing for {blueprint:?}");
        }
    }

    #[test]
    fn nothing_recognisable_attaches_nothing_rather_than_a_default() {
        let matched = match_skills(Some("everything is fine"), "what is a Blueprint?", None, 3);
        assert!(matched.is_empty());
        assert!(match_skills(Some("No space left on device"), "", None, 0).is_empty());
    }

    /// The trigger lists are what makes this work at all, so a typo that
    /// makes one unreachable should fail here rather than in production.
    #[test]
    fn every_skill_is_reachable_by_at_least_one_of_its_own_triggers() {
        for skill in SKILLS {
            assert!(!skill.triggers.is_empty(), "{} has no triggers", skill.id);
            for trigger in skill.triggers {
                assert_eq!(trigger.to_lowercase(), **trigger, "{}'s trigger {trigger:?} must be lowercase to ever match", skill.id);
            }
            let blueprint = skill.blueprints.first().copied();
            let matched = match_skills(Some(skill.triggers[0]), "", blueprint, 5);
            assert!(ids(&matched).contains(&skill.id), "{} cannot be reached by its own first trigger", skill.id);
        }
    }

    #[test]
    fn ids_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for skill in SKILLS {
            assert!(seen.insert(skill.id), "duplicate skill id {}", skill.id);
        }
    }
}

