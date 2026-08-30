# agent-install

`install.sh` is the Etap F installer: it fetches, verifies, and installs
`vibe-agent` as a systemd service on a Linux host. It does not implement any
agent logic itself (no pairing, no config parsing) - see the project rules
in the main planning doc: shell scripts stay dumb on purpose.

## Normal use

```sh
curl -fsSL <release-url>/install.sh | sudo sh
```

Then, on the same machine:

```sh
vibe-agent pair <CODE-SHOWN-IN-VIBESSH>
```

`<release-url>` isn't live yet - `VibeSSH/vibessh` has no published releases
and is currently a private repo, so there's nothing for `curl` to fetch
until that changes. `BASE_URL` already points at the real place releases
will eventually live (`github.com/VibeSSH/vibessh/releases`) rather than a
placeholder domain.

## Testing without a published release or a Linux box

Two separate things are testable independently of that:

**Pure logic** (`detect_arch`, `detect_os`, `verify_checksum`) - runs
anywhere with a POSIX shell, including this Windows dev machine:

```sh
sh agent-install/test.sh
```

This is real verification, not a simulation: `detect_os` is asserted to
*reject* Linux-only detection because the machine running the test genuinely
isn't Linux, and `verify_checksum` is checked against real `sha256sum`
output for a real temp file. It does **not** touch `useradd`, systemd, or
the network - those steps have no meaningful equivalent to test outside
Linux.

**Everything else** (user creation, directories, the systemd unit, starting
the service) needs a real Linux VM or container. On one, skip the
download/checksum step and install an already-built binary instead:

```sh
sudo VIBESSH_INSTALL_LOCAL_BINARY=/path/to/vibe-agent ./install.sh
```

This exercises the exact same `ensure_service_user` / `ensure_directories`
/ `write_unit` / `start_service` code path the real one-liner uses - only
the fetch step is swapped out.

## Environment variables

| Variable                        | Default                                      | Purpose                                    |
|----------------------------------|-----------------------------------------------|---------------------------------------------|
| `VIBESSH_INSTALL_REPO`          | `VibeSSH/vibessh`                            | GitHub repo releases are fetched from      |
| `VIBESSH_INSTALL_VERSION`       | `latest`                                     | Release tag, or `latest`                   |
| `VIBESSH_INSTALL_BASE_URL`      | `https://github.com/<repo>/releases`         | Override for a different release host      |
| `VIBESSH_INSTALL_BIN_DIR`       | `/usr/local/bin`                             | Where `vibe-agent` is installed            |
| `VIBESSH_INSTALL_CONFIG_DIR`    | `/etc/vibessh/agent`                         | Config directory (created, owned by service user) |
| `VIBESSH_INSTALL_DATA_DIR`      | `/var/lib/vibessh/agent`                     | Data directory (identity, paired credential hash) |
| `VIBESSH_INSTALL_USER`          | `vibessh-agent`                              | Dedicated system user the service runs as  |
| `VIBESSH_INSTALL_UNIT_PATH`     | `/etc/systemd/system/vibessh-agent.service`  | Where the unit file is written             |
| `VIBESSH_INSTALL_LOCAL_BINARY`  | (unset)                                      | Skip download/checksum, install this file instead |

## Why these paths

`/usr/local/bin`, `/etc/vibessh/agent`, `/var/lib/vibessh/agent` follow the
Filesystem Hierarchy Standard as-is: `/usr/local/bin` for a binary not
managed by the distro's package manager, `/etc/<app>` for configuration,
`/var/lib/<app>` for persistent state that isn't a cache or a log. No
deviation from what the planning doc suggested was needed.

## What isn't done here

- **Signature verification.** Only a SHA-256 checksum is checked. Real
  signing needs a release pipeline and a key that don't exist yet - tracked
  as an Etap K (security review) follow-up, not silently skipped.
- **Deep privilege analysis.** The systemd unit already runs as a
  non-root dedicated user with baseline hardening
  (`NoNewPrivileges`, `ProtectSystem=strict`, `ProtectHome`), but which
  *specific* agent features need more than that (Docker socket access,
  restarting other systemd units, etc.) and what capability/polkit/sudo-helper
  model covers that gap is Etap G's job, done as a deliberate follow-up.
