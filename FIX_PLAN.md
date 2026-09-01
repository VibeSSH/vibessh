# VibeSSH — Fix Plan

Companion to `AUDIT_REPORT.md` (commit `bb04132`). Phases are ordered by risk, not by convenience. Phase A must land before any release build is cut.

## Working rule for every fix

1. Write the failing test / reproduction **first**.
2. Apply the fix.
3. Confirm the test passes.
4. Run `cargo test --workspace` + `cargo clippy --workspace --all-targets` + `npx tsc --noEmit` for regressions.

If a problem cannot be sensibly tested (e.g. a doc correction), say so explicitly in the commit message rather than skipping the test silently.

**Blocker:** F-001 must be fixed first. Until `src-tauri/tests/vibe_network.rs` compiles, step 3 is impossible for anything in this plan.

---

## PHASE A — CRITICAL security and reliability (release blockers)

Nothing ships until every item here is closed.

### A.0 — Unblock the test suite
| # | Item | Finding | Files |
|---|---|---|---|
| A.0.1 | Fix the three call-site signature mismatches | F-001 | `src-tauri/tests/vibe_network.rs:178,181,189` |

*Test:* `cargo test --workspace --no-run` exits 0.

### A.1 — Shell-injection root cause
The single highest-leverage change in the whole plan. Four CRITICALs share this cause.

| # | Item | Finding | Files |
|---|---|---|---|
| A.1.1 | Create `src-tauri/src/ssh/command.rs`: one `ShellCommand` builder with a single `shell_quote`, a single `reject_unsafe` (blocking `\n \r $ ` ` ( ) { } \` and null bytes), and typed validators for hostname, IP, CIDR, WireGuard key, port, octal mode, Linux username, Docker image ref | S-002, duplication | new file |
| A.1.2 | Delete all six duplicate `shell_quote` copies and the three `reject_*` variants; migrate every call site | duplication | `dedicated_user.rs`, `files/sudo_user.rs`, `runtime/docker.rs`, `services/{application,database,dns}_service.rs`, `runtime/remote_process.rs` |
| A.1.3 | Quote the WireGuard heredoc (`<<'VIBESSH_WG_EOF'`); substitute the private key on the Node from a placeholder instead of shell expansion | **S-002** | `network/wireguard.rs::build_apply_script` |
| A.1.4 | Validate every peer field as base64 key / hostname-or-IP before it enters a config | **S-002** | `network/wireguard.rs` |

*Tests:* `build_apply_script` rejects `$(`, backtick, `${` in `public_key`, `allowed_ip`, `endpoint`; snapshot asserts the delimiter is quoted; a `server.host` of `` $(id) `` is rejected at `create_server`.

### A.2 — Privilege and isolation boundaries
| # | Item | Finding | Files |
|---|---|---|---|
| A.2.1 | Replace `/tmp/vibessh-wg-strip.conf` with `mktemp` under a root-only `/run/vibessh/` (0700), or use `wg syncconf <(wg-quick strip …)` | **S-003** | `network/wireguard.rs` |
| A.2.2 | Move the console FIFO out of the bind-mounted directory to `/run/vibessh/<app-id>.stdin`, mode 0600, owned by the connecting admin. Remove `chmod 666` | **S-004** | `runtime/docker.rs::build_attach_script` |
| A.2.3 | Move file staging from `/tmp` to a per-Application `/run/vibessh/<app-id>/` (0700, owned by that Application's account). Remove `chmod 644` | **S-005** | `files/sudo_user.rs` |
| A.2.4 | Add a `cleanup` op to the helper script so the unlink runs **as the owning user**; check the result and log on failure instead of `let _ =` | **S-005** | `files/sudo_user.rs` |
| A.2.5 | Validate `working_directory` at creation: absolute, under an allowed prefix, not a system path, no `..`. Move the `chown -R` to a once-only provisioning step guarded by a marker file; stop discarding its error | **S-012** | `services/application_service.rs::create_application`, `runtime/docker.rs` |

*Tests:* helper-script shape assertions (no fixed `/tmp` path, no world-readable/writable mode); integration test that no `vibessh-stage-*` survives a read/download/upload cycle; `create_application` rejects `/`, `/etc`, `/home`, relative paths.

### A.3 — Network exposure
| # | Item | Finding | Files |
|---|---|---|---|
| A.3.1 | `resolve_bind_address(VibeNetwork)` returns the Node's **mesh IP**, never `0.0.0.0`. `Localhost` keeps `127.0.0.1`. Only `Public` may bind `0.0.0.0` | **S-001** | `services/application_service.rs` |
| A.3.2 | Write explicit `DOCKER-USER` iptables rules alongside UFW rules for every published Docker port; stop treating UFW alone as enforcement for containers | **S-001** | `firewall/` (new `docker_user.rs`) |
| A.3.3 | Distinguish "no firewall backend" from "synced" in `FirewallSyncResult`; refuse non-public visibility on a Node with no enforcement mechanism; stop downgrading hard errors to `log::warn!` | **S-014** | `firewall/mod.rs`, `services/firewall_service.rs`, `services/application_service.rs::sync_firewall_best_effort` |
| A.3.4 | Before `ufw --force enable`, determine the **live** SSH server port (`$SSH_CONNECTION` / `ss -tnp`) and refuse to enable unless a rule covers it. Show the exact port in the confirmation UI | **S-013** | `firewall/ufw.rs::enable`, `pages/Firewall.tsx` |

*Tests:* `resolve_bind_address(VibeNetwork)` never returns `0.0.0.0`; `reconcile_node` with no provider returns an "unenforced" variant; `enable` refuses when the live port is uncovered.

### A.4 — Database exposure
| # | Item | Finding | Files |
|---|---|---|---|
| A.4.1 | Delete `ensure_mysql_listens_on_all_interfaces` entirely. Reach MariaDB over `127.0.0.1` or the Docker gateway | **S-006** | `services/database_service.rs` |
| A.4.2 | Replace `CONNECTIONS_FROM = "%"` with the Docker subnet or `localhost`; make it explicit per database host | **S-006** | `services/database_service.rs` |
| A.4.3 | Make DB-server installation an explicit, consented, progress-reported action; stop discarding its errors | S-006, S-033 | `services/database_service.rs`, new UI step |
| A.4.4 | Escape backslashes in `sql_quote` (or switch to parameterised statements) and restrict `admin_username`/`host` at input validation | **S-009** | `services/database_service.rs` |
| A.4.5 | Stop embedding secrets in command strings: `--defaults-extra-file` for mysql, stdin for `docker login` | **S-008** | `database_service.rs::build_mysql_command`, `application_service.rs::ensure_registry_login` |

*Tests:* no grant string contains `'%'`; no code path writes a `bind-address` file; `sql_quote("x\\")` and `sql_quote("a'; DROP …")` produce inert literals; no built command string contains a secret value.

### A.5 — Resource teardown
| # | Item | Finding | Files |
|---|---|---|---|
| A.5.1 | Rewrite `delete_application` as a real teardown: stop → `runtime.destroy()` → revoke firewall → delete DNS record → drop databases → remove FIFO → remove dedicated user → (optional, confirmed) remove working dir → delete row. Report partial failures explicitly | **S-007** | `services/application_service.rs`, `commands/application_commands.rs` |
| A.5.2 | Add the same teardown to Server deletion for every Application on that Node | D-006 | `services/server_service.rs` |

*Tests:* delete-then-recreate on the same external port succeeds; after delete, no `vibessh-app-<old-uuid>` container, no dedicated user, no firewall rule remains.

### A.6 — Startup and crash resilience
| # | Item | Finding | Files |
|---|---|---|---|
| A.6.1 | Migrate once at startup, before any repository opens; set `PRAGMA journal_mode=WAL` and `PRAGMA busy_timeout=5000` on every connection; share one pool | **A-004 / D-001** | `src-tauri/src/lib.rs`, all 9 repositories |
| A.6.2 | Replace every `expect()` on stored DB values with a fallible `row_to_*` that skips + logs a malformed row | **A-005** | `storage/server_repository.rs:426,454,459`, `storage/application_repository.rs:569,573` |
| A.6.3 | Cap declared entry size, total extracted bytes and entry count in `extract_zip`; stream through a `take()`-limited reader | **S-011** | `files/archive.rs` |
| A.6.4 | Skip symlinks and track visited canonical paths in `collect_for_zip` | **S-017** | `files/archive.rs` |

*Tests:* concurrent-write test across two repositories on one path; repository reads against corrupted UUID / timestamp / capabilities JSON return errors, not aborts; a crafted zip declaring an oversized entry is rejected; `create_zip` over a self-referential symlink terminates.

### A.7 — Agent transport
| # | Item | Finding | Files |
|---|---|---|---|
| A.7.1 | Implement TOFU certificate pinning for Agent Mode, matching the SSH path: store the fingerprint on first successful pairing, reject mismatches thereafter. Remove `danger_accept_invalid_*` | **S-010** | `src-tauri/src/agent_client/mod.rs`, `storage/server_repository.rs` |
| A.7.2 | Constant-time comparison for the pairing code | S-022 | `agent/src/pairing/mod.rs::try_consume` |

*Tests:* a differing certificate is rejected; the first connection stores the fingerprint; pairing-code comparison uses `subtle`/`constant_time_eq`.

**Phase A exit criteria:** all 7 CRITICALs closed with tests; `cargo test --workspace` green; `cargo clippy --workspace --all-targets` with zero errors; `npm audit` and `cargo audit` clean or explicitly waived with justification.

---

## PHASE B — HIGH bugs and correctness

| # | Item | Finding |
|---|---|---|
| B.1 | Narrow `retry_on_connection_failure` to `AppError::Connection` only; surface the second error when both attempts fail; rename it accurately | A-003 |
| B.2 | Add an atomic `get_or_connect` to `SshSessionManager` (per-server `OnceCell`/mutex); call `close()` in `remove()` | P-006 |
| B.3 | Add a per-command timeout to `execute_command`; reconcile it with `inactivity_timeout` (raise or remove the 60 s value for long operations) | P-004 |
| B.4 | Cap `execute_command` stdout/stderr accumulation | P-005 |
| B.5 | Replace `merge_new_log_lines` content-matching with Docker's `--since` timestamp | P-009 |
| B.6 | Make `LogCaptureStore` append-only with an atomic write-and-rename and a per-application lock; make `tail` read from the end | P-002 |
| B.7 | Fail `leave_node` loudly when `wireguard::teardown` fails; do not remove the row on failure; call `sync_dns` after leaving | S-026, S-027 |
| B.8 | Enforce DNS hostname uniqueness across Node aliases *and* Application aliases; reject or disambiguate collisions at creation | S-028 |
| B.9 | Add a lock and an atomic rewrite for `/etc/hosts`; fail safe if `tee` fails after `sed` | S-025 |
| B.10 | Roll back or report partially-applied firewall rules | S-030 |
| B.11 | Stop the Application (with confirmation) and stage-then-swap on restore | S-016 |
| B.12 | Stream backups: build the archive on the Node, stream to S3/local; never materialise in desktop memory | S-015 |
| B.13 | Upload to a `.part` path and rename on success; clean up on abort | S-031 |
| B.14 | Reject `http://` S3 endpoints (or require explicit opt-in) | S-023 |
| B.15 | URL-encode the `?db=` parameter in `phpmyadmin_url` | S-024 |
| B.16 | Surface keyring deletion failures instead of `let _ =` | S-036 |
| B.17 | Decide S-018: per-Application Docker networks with opt-in links, **or** document the shared network as a visible trust boundary in the UI. Do not leave it implicit | S-018 |

---

## PHASE C — Tests

| # | Item |
|---|---|
| C.1 | Add a frontend test framework (vitest + testing-library) and `test` / `lint` / `typecheck` scripts to `package.json` |
| C.2 | Security test suite: path traversal, symlink escape, zip bomb, malicious archive paths, shell metacharacters into WireGuard/DNS/database/Docker, malicious env keys, invalid DNS/Node/Application/image names, Unicode, very long input, null bytes, ANSI sequences in logs and terminal output |
| C.3 | Scenario matrix for every service method: SUCCESS / INVALID INPUT / PERMISSION DENIED / NOT FOUND / TIMEOUT / CONNECTION LOST / PARTIAL FAILURE / RETRY / CONCURRENT / CANCEL / RESTART |
| C.4 | Port-collision test (DB owner + live listener + concurrent add) |
| C.5 | Docker lifecycle tests: create → start → config change → recreate → destroy; orphan detection; rollback on failed recreate |
| C.6 | Concurrency tests: double-click start/stop, concurrent DNS sync, concurrent firewall reconcile, concurrent file writes, concurrent repository writes |
| C.7 | `proptest` for `sandbox::sanitize_relative_path`, `dns_service::normalize_alias`, `ufw::parse_added_rules`, `docker::parse_docker_byte_size`, `firewall_service::parse_ss_output` |
| C.8 | CI: a GitHub Actions workflow running clippy (warnings denied), `cargo test --workspace`, `tsc --noEmit`, `npm run lint`, `npm audit`, `cargo audit`. **The repository currently has no CI at all.** |

---

## PHASE D — Performance

| # | Item | Finding |
|---|---|---|
| D.1 | Gate all polling on `document.visibilityState`; pause when the window is hidden | P-001 |
| D.2 | Consolidate the five interval constants into one config module; consider a single backend-pushed event stream instead of five independent pollers | P-001 |
| D.3 | Batch `create_directory_all` into a single remote `mkdir -p` per archive rather than a `metadata` call per segment per file | P-003 |
| D.4 | Cache `ensure_registry_login` per (Node, registry) for the session | P-007 |
| D.5 | Virtualize file, container, process and log lists | P-008 |
| D.6 | Reduce `docker inspect` / `docker stats` round trips — one batched call per Node rather than per Application | P-001 |

---

## PHASE E — Architecture cleanup

| # | Item | Finding |
|---|---|---|
| E.1 | Implement the error taxonomy from `AUDIT_REPORT.md` §10; serialize `{ code, params, detail }` | S-020 |
| E.2 | Preserve `code` and `params` through `tauri.ts::normalizeError`; render via `t("errors.<code>", params)` with `detail` behind a disclosure | S-019, U-004 |
| E.3 | Decide `ServerConnection`: delete it, or make it the real seam and implement `AgentTransport` | A-001, A-002 |
| E.4 | Remove the confirmed dead code (`backend/tests/common` helpers, `constants/permissions.ts`, `current_rules` if still unused after E.3) | §12 |
| E.5 | Unify the two `slugify` implementations | §12 |
| E.6 | Replace the four copy-pasted connect-retry blocks with the (now-corrected) `retry_on_connection_failure` | §12 |
| E.7 | Split `application_service.rs` (1700 lines) along its natural seams: lifecycle / ports / config / registry / logs | A-001 |
| E.8 | Add `down` migrations and a migration-checksum guard | D-002, D-003 |
| E.9 | Add a unique constraint on `application_ports (server_id, protocol, external_port)` | D-007 |

---

## PHASE F — Frontend and UI polish

| # | Item | Finding |
|---|---|---|
| F.1 | One accessible `<Modal>` primitive: focus trap, focus restore, `role="dialog" aria-modal="true"`, labelled heading, Escape. Migrate all 20+ modals | U-001 |
| F.2 | `aria-label` on every icon-only control | U-002 |
| F.3 | "Configuration pending" state after env/image/limits/port changes, with a Recreate banner explaining the restart | U-003 |
| F.4 | Replace every "success on partial failure" path with an accurate outcome state | U-005 |
| F.5 | Run the visual pass at 1920/1600/1440/1366/1280/1024/900/800; re-validate `docs/UI_AUDIT.md` against current code | U-006 |
| F.6 | Keyboard navigation and tab-order review; contrast check; ensure no status is conveyed by colour alone | §9 |

---

## PHASE G — Documentation

| # | Item | Finding |
|---|---|---|
| G.1 | Re-run and re-date `docs/security-review.md` — its central claim ("nothing shells out") is now false | §13 |
| G.2 | Update `docs/agent-privileges.md` to describe the actual `sudo docker` path | §13 |
| G.3 | Verify `README.md` feature claims line-by-line against code | §13 |
| G.4 | Document the trust boundaries from `AUDIT_REPORT.md` §4 as a maintained threat model | §4 |
| G.5 | Add `CONTRIBUTING`/`AGENTS.md` covering the shell-command rule (A.1), the "no secrets in argv" rule (S-008), and the "no `let _ =` on user-visible operations" rule | §12 |

---

## Suggested sequencing

- **A.0 → A.1** first: the test suite must build, and the shared command builder is a prerequisite for A.2–A.4.
- **A.6.1** (SQLite) is independent and can run in parallel — it is also the most likely cause of any intermittent startup failure a user reports today.
- **A.5** (teardown) depends on nothing but is the largest single behavioural change; give it its own PR and its own integration test.
- Phase C.8 (CI) is worth pulling forward to the end of Phase A so that Phase B onward is regression-protected automatically.
