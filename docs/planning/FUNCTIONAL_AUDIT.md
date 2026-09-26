# Functional audit - does each feature actually work?

September 2026. Written after Node-to-Node migration turned out to have never
worked across twenty releases. `AUDIT_REPORT.md` asked whether the code is
safe; this asks whether each thing the panel offers does what it says on a
real Node, as root **and** as a non-root admin with passwordless sudo.

**How it was done.** Five read-only reviews, one per area, each tracing every
user action from the button through the service call, the registered Tauri
command and the backend route to the exact shell run on the Node. The most
consequential findings were then re-checked by hand before being written
down here; those are marked **confirmed**. Everything else is a finding from
reading the code and says what live test would settle it.

**What was fine.** All 256 commands the UI calls are registered, every
argument name matches after Tauri's camelCase conversion, and every backend
request and response field matches the desktop models. No button is dead for
lack of wiring. Every problem below is in what a command *does*.

**The one pattern behind most of it.** Almost all Node-side code is tested by
asserting on the command string it builds. Nothing runs it. The failures
below are the kind that only running finds: a directory the admin cannot
write, a binary not on a non-root PATH, a symlink followed, an exit code
nobody read, a result the UI never looks at.

Severity: **P0** loses data or gives root; **P1** a feature does not work or
reports success while failing; **P2** works in the common case, breaks at
the edges.

---

## P0 - data loss and privilege

**All seven, and the sharing bug below, are fixed** - see the commits of the same day. Each fix is tested by running the operation where that was possible (real symlinks, the real helper script in WSL, the retention decision, the migration refusal); the live pass below still has to confirm them on a Node.

| # | Finding | Where | Status |
|---|---|---|---|
| 1 | **Deleting a symlink deletes its target.** SFTP `REALPATH` and the helper's `realpath` resolve before delete/rename/upload-over. On the Node Files page (root provider `/`) deleting `/bin` on Ubuntu 24.04 empties `/usr/bin`; deleting `sites-enabled/default` deletes `sites-available/default`; uploading over a symlinked `server.jar` deletes its target. | `files/sftp.rs:46-66`, `files/sudo_user.rs:147-157` | **confirmed** |
| 2 | **Any team member can become root at the next access sync.** The sync writes into the member's own `~/.ssh` as root (`sudo install -d`, `sudo tee authorized_keys.new`, `sudo chown`, `sudo chmod`), all of which follow symlinks the member controls. `ln -s /etc/passwd ~/.ssh/authorized_keys.new` and wait. Includes members with no permissions, and removed members still logged in (revoke reuses the code). | `member_account.rs:66, 94-96` | reviewed; live test on a disposable VM |
| 3 | **Recreate throws away limits and image changes.** Recreate re-renders `runtime_config` from the creation-time wizard answers and stores it; memory/CPU/disk limits and a changed image live only in `runtime_config`, so they vanish. It runs automatically after saving limits, env or ports on a running app. The disk limit disappears from the UI while `/etc/cron.d` keeps enforcing it. | `application_service/lifecycle.rs:135`, `provisioning.rs:315-333, 476` | **confirmed** |
| 4 | **Recreate SIGKILLs a running server.** `destroy` is `docker rm -f` with no graceful stop, so an env edit on a running Paper server kills it mid-save. Stop/restart use Docker's 10 s default, not the 120 s the schedule runner uses. | `runtime/docker.rs:1379, 1207` | reviewed; live |
| 5 | **Backup retention by size deletes the backup it just made, every 15 minutes**, while toasting "backup created"; retention also prunes manual backups. Scheduled-backup failures leave no trace at all. | `application_backup_service.rs:339, 391`; `useBackupScheduler.ts` | reviewed |
| 6 | **Pterodactyl import as a non-root admin copies nothing and reports success** - Wings volumes are unreadable, the `cp` exit code is never read, the empty count parses as 0. The operator starts a fresh world. | `pterodactyl_run_service.rs:174-181` | reviewed; live |
| 7 | **Migration silently drops the app's databases, backups, schedules and connections** (cascade on the source row); the database stays on the old Node, untracked, and the migrated app cannot reach it. A failed migration leaves the source stopped. The source is stopped based on stored status, not a fresh one. | `migration_service.rs:195, 239, 297, 489`; `teardown.rs:44-47` | reviewed |

## P1 - features that do not work, or say they did

**Team access and per-application permissions**
- **Sharing never records the Node**, so per-application permissions are never written to a Node and no member ever sees a shared app. Both share sites pass `teamServerId: null`. *(Introduced with per-application permissions; confirmed.)* `ApplicationMembersTab.tsx:210`, `ApplicationsSection.tsx:59`.
- A member's file transfers (download, upload, large view, extract, new file) need admin-only staging steps. `files/sudo_user.rs:519-589`.
- Revoking a member's only device leaves its key on every Node: sync skips members with no keys. `team_access_service.rs:136`.
- No way to add an existing account to a team (backend route exists, no client). Provisioning tells the user to "add them as a member instead".
- Deleting a team server or a team leaves member accounts on Nodes, untracked.

**Firewall and network**
- **DOCKER-USER drops block legitimate outbound container traffic.** The DROP has no ingress-interface or `--ctstate DNAT` match, so a "Vibe Network only" port N stops every container on that Node from reaching port N anywhere. `firewall/docker_user.rs`.
- **Non-root admin on Debian: ufw is "missing"** - `/usr/sbin` is not on a non-root SSH exec PATH. Setup says missing, install errors, the Firewall page has no backend, DOCKER-USER is never reconciled, and member firewall rules are dropped. `firewall/ufw.rs:164`, `server_service.rs:95,150`, `member_account.rs:180`.
- Join reports success when the tunnel never came up; the mesh does not survive a reboot (`wg-quick@` never enabled); leave is not a full teardown; private DNS names do not resolve inside containers.
- Custom firewall rules are not validated (and the CIDR goes unquoted into the command); a bad one reports success and then breaks every later sync of that Node.
- Enforcement failures never reach the UI (`containerError`/`unenforced` dropped).
- Agent mode: never reconnects after a desktop restart, the upgrade has no way back, "Secure" closes the agent port.

**Applications**
- **systemd runtime does not work for a non-root admin** (SFTP into `/etc/systemd/system`, `daemon-reload` without sudo); systemd and remote_process apps are not torn down on delete. `runtime/systemd.rs:248-266`.
- Docker console goes deaf after a Node reboot (`/run` wiped, only exit 124 re-attaches). `runtime/docker.rs:1494`.
- Docker command failures are treated as dropped connections: a port clash drops the SSH session and kills every console and stream on the Node. `ssh_service.rs:98`.
- Health checks probe `internal_port`, so a healthy app with a remapped or non-public port shows Unhealthy.
- Working-directory validation lets `/home/<admin>` and `/etc/ssh` through; a dedicated-user start then `chown -R`s it.

**Files, terminal, forwarding**
- Dedicated-user transfers stage whole files in `/run` (tmpfs, ~10 % of RAM): anything over a couple of hundred MB fails, and smaller files take RAM from the game server.
- Terminal loses its first output (MOTD, prompt); one failed terminal open drops the whole Node session; multi-byte characters split across chunks.
- After a Node reboot, the Node Files page, Monitor, Dashboard, Actions and port forwards stay on the dead session; a dead forward still shows as running.
- chmod on a folder reports failure after succeeding (full `setstat` truncates first).

**Data**
- Database reachability repair fails on Ubuntu 22.04 / Debian 11 (MariaDB < 10.11), on MySQL, and for a non-root admin on Debian - and reports success.
- Backups as a non-root admin abort on one unreadable file; restore overlays rather than replaces; S3 holds the whole archive in memory and fails above 5 GiB.

**Account backend**
- "Active installs" and the sign-in rate limit key on `ip:unknown` unless `TRUST_FORWARDED_FOR` is set behind the tunnel - every user shares one counter. *(Check the production env.)*
- A freshly provisioned member publishes no device key until they restart the app.

## P2 - edges

Secrets: a missing keyring entry becomes an empty password; remote_process
and Redis/NATS put secrets in argv. Log snapshot interleaving lost. Editor
"atomic" save never atomic on OpenSSH. Cancelled transfers leak staging files
and SFTP handles. Extract costs ~6 SSH round trips per file. WireGuard/ufw
installs are apt-only with no dpkg-lock wait. The agent installer needs
OpenSSL 3. A refused refresh leaves the UI "signed in". The cloud HTTP client
has no timeout and holds a mutex for the whole request.

---

## Order of work

1. **P0 #1-#3 and the share fix** - symlink-safe delete/rename/upload, the
   access sync writing through the member's home safely (no root writes into
   paths the member controls), recreate keeping `runtime_config` edits, and
   sharing recording the Node. Each gets a test that runs the real operation,
   not a string assertion.
2. **Silent success** - every place above where the UI is told "done" while
   the Node disagrees: backups, Pterodactyl copy, DB repair, network join,
   firewall enforcement, teardown warnings.
3. **Non-root admin** - one PATH fix (`sudo` resolution or a full PATH on the
   SSH exec channel) closes several Debian findings at once; then systemd.
4. **Live test harness** (below) - so the next regression of this kind fails
   a test instead of reaching a user.
5. The rest of P1, then P2.

## Live testing

Chosen: local, in Docker. Two systemd-enabled Ubuntu 24.04 containers on a
private Docker network, each with sshd, Docker-in-Docker and a root and a
non-root sudo admin - two Nodes with their own addresses, for migration and
Vibe Network. A scripted pass drives the app's own services against them
(the `#[ignore]` integration-test style that already exists) and records a
verdict per step. Kernel-level behaviour - WireGuard, iptables inside nested
Docker, reboot survival - is only approximate there and is marked as such;
those steps want a real VPS before they are called verified.

Each area review listed concrete live steps; they are the checklist for that
pass.
