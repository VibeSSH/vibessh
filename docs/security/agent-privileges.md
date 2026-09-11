# Agent privilege analysis (Etap G)

The agent daemon runs as its own unprivileged system user (`vibessh-agent`,
no login shell, no home directory) - not root, per the project rule "Nie
uruchamiaj całego agenta jako root bez uzasadnienia." This documents *why*
each planned feature does or doesn't need more than that, and what
mechanism covers the gap when it does. Nothing described as "future" here
exists in the agent's code yet - this is the analysis Etap G asks for,
prepared ahead of those features so the privilege model doesn't get bolted
on as an afterthought once they land.

## Already implemented, needs nothing extra

- **Identity, handshake, pairing.** File I/O under the agent's own
  `/var/lib/vibessh/agent` and `/etc/vibessh/agent`, both owned by
  `vibessh-agent`. No elevation of any kind.
- **Realtime metrics** (CPU/RAM/disk/load/uptime/network - not built yet,
  but analyzed now since it's next in line). Reading `/proc` and `/sys` for
  system-wide (not per-other-user) stats needs no special privilege on
  Linux.

## Needs elevation, and how

### Systemd unit management (Quick Actions: restart nginx/a game server/...)

Systemd's D-Bus API enforces polkit authorization for starting, stopping,
restarting, enabling, or disabling units outside the caller's own session -
an unprivileged user gets `Access denied` by default, full stop.

**Solution: a polkit rule with a root-owned allowlist file**, installed by
`agent/install/install.sh` (`install_polkit_rule`, `ensure_directories`).
The rule (`/etc/polkit-1/rules.d/49-vibessh-agent.rules`) authorizes
`vibessh-agent` to `start`/`stop`/`restart`/`try-restart`/`reload*` and
`enable`/`disable` - but only for unit names listed in
`/etc/vibessh/managed-units.conf`, and never `kill` (arbitrary signals) or
`set-property` (arbitrary resource-limit changes) even for allowed units.
Empty file by default = the agent can manage zero units until an admin
explicitly adds some.

The one subtlety worth writing down: **the allowlist directory cannot be
owned by `vibessh-agent`.** If it were, the agent could delete and recreate
the (root-owned) allowlist file itself - directory *write* permission
controls who can replace an entry, independent of that entry's own
ownership. That's why `/etc/vibessh` (root:root, 0755, holds
`managed-units.conf`) is a separate directory from `/etc/vibessh/agent`
(vibessh-agent:vibessh-agent, 0750, the agent's own writable config space),
not a parent/child pair both owned by the service user.

Until a Quick Actions setup flow exists to manage this file through some
other privileged channel, an admin edits it by hand. That's an intentional
v1 limitation, not an oversight - the alternative (letting the unprivileged
agent process manage its own authorization scope) would defeat the point of
gating it at all.

### Docker (containers list/start/stop/restart/logs)

Membership in the `docker` group is, in practice, root-equivalent: the
Docker socket lets its members mount the host filesystem into a container
and escape confinement. Granting that to `vibessh-agent` now, before any
Docker feature exists to use it, would be handing out root-equivalent
access on spec.

~~**Deferred, deliberately**~~ — **the decision was made elsewhere, by
default, and this document did not record it.**

Docker support shipped. It did not go through the agent at all: every Docker
operation runs `sudo docker ...` over the **connecting SSH admin's** session
(`runtime::docker`, `ssh::docker`). The agent still has no `docker` group
membership, so the letter of the decision above holds — but the practical
outcome is the thing it was written to avoid. `sudo docker` is exactly as
root-equivalent as `docker` group membership; VibeSSH simply reaches it
through an identity that already had broad `sudo` rather than by widening
the agent's.

That is a defensible choice — it is the admin's own authority being used for
host management, not a new grant to a long-running daemon — but it is a
choice, and it deserves to be written down rather than left looking
deferred. Two consequences follow that the original framing hides:

- **Every Docker feature requires an SSH-mode Node.** An Agent-mode Node
  cannot run Applications at all (`AUDIT_REPORT.md` A-002), and this is one
  of the reasons why.
- **VibeSSH assumes passwordless `sudo` on every managed Node.** Not just
  for Docker: `ufw`, `wg-quick`, `useradd`, `install`, `mysql` and the file
  helper all rely on it. That requirement is not stated in
  `agent/install/README.md` or the setup flow, and a Node whose admin
  account prompts for a password fails in confusing ways rather than being
  refused up front.

The options this section listed for the agent — opt-in group membership, or
a narrow proxy in front of the Docker API — are still the right ones to
compare *if* Agent-mode Applications are ever built. They are not decided,
and nothing depends on them today.

### Cross-user process/file access (e.g. a Minecraft server under its own UID)

Killing a process or reading/writing files owned by a different Linux user
needs root (or that user's own privileges) - `vibessh-agent` has neither by
design.

**Intended pattern, not yet built**: a small, separately-audited helper
binary invoked via a tightly scoped `/etc/sudoers.d/vibessh-agent` rule
(`NOPASSWD` for specific, parameterized commands only - e.g.
`vibessh-agent-helper kill-process <pid>`, where the helper itself
validates the target before acting, not a blanket
`vibessh-agent ALL=(ALL) NOPASSWD: ALL`). Not implemented because nothing
calls it yet - process management and Minecraft server management are both
still unbuilt features. Building the helper before there's a caller would
mean shipping privileged code with no real usage to validate its scope
against.

### Terminal (execute arbitrary commands)

Worth naming the asymmetry explicitly: in **SSH Mode**, a command runs as
whichever user the desktop authenticated as (often root on a VPS) - full
shell access is the point. In **Agent Mode**, every command runs as
`vibessh-agent` regardless of who's driving the desktop app, because that's
the one identity the daemon has. Agent Mode's terminal will always be more
restricted than SSH Mode's by construction, not as a bug - it's the
tradeoff for not requiring the user's own credentials to be handed to a
long-running daemon. Anything Agent Mode's terminal needs beyond what
`vibessh-agent` can already do goes through the same sudo-helper pattern as
process/file access above, evaluated per-command once that feature exists.

## What's verified for real, not just designed

The polkit rule and directory-ownership split were tested against a real
`polkitd` on Ubuntu 24.04 (see the project's dedicated test server), not
just reasoned about:

- `vibessh-agent` denied restarting a unit with an empty allowlist
- allowed after adding that unit to `managed-units.conf`
- still denied for units *not* in the allowlist, with another unit present
- confirmed `vibessh-agent` cannot write `managed-units.conf` or replace it
  via its parent directory
