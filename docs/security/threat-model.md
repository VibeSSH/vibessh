# Threat model

What VibeSSH is protecting, from whom, and where the boundaries actually
are. Maintained — unlike `security-review.md`, which is a snapshot of one
review and has been superseded.

Derived from the full audit (`AUDIT_REPORT.md` §4) and updated as the
remediation lands. **Update this when you add a feature that crosses a
boundary**, not afterwards.

---

## Assets

| Asset | Where it lives | Protected by |
|---|---|---|
| SSH passwords and key passphrases | OS keyring | Never in SQLite. Verified across all 15 migrations. |
| Agent bearer credentials | OS keyring (desktop); SHA-256 hash only (agent) | Constant-time comparison; the raw value never persists on the Node. |
| WireGuard private keys | `/etc/wireguard`, mode 0600, on each Node | Generated on the Node, never transmitted. `wg-quick strip` output stages in a root-only directory. |
| Application files and databases | The Node | Per-Application Linux account; path sandboxing with post-canonicalisation checks. |
| An Application's unpublished ports | The Node's Docker networking | Its own private bridge network. Another Application reaches it only through a connection granted in the UI, which is a second private network holding exactly those two containers. |
| Backup archives and S3 credentials | The Node, plus the configured bucket | HTTPS enforced for non-loopback endpoints. |
| Registry tokens | OS keyring | Reach the Node through a mode-0600 file, never a command line. |
| Docker daemon control | The Node | `sudo docker` as the connecting admin. **Root-equivalent** — see `agent-privileges.md`. |
| Cloud backend JWT secret | `apps/backend/.env` | Minimum length enforced at startup. |

## Trust boundaries

```
[Desktop app]  ──SSH, TOFU-pinned──────────────►  [Node: admin account, broad sudo]
      │                                                      │
      └────────wss, TOFU-pinned───────────────►  [Vibe Agent: unprivileged user]
                                                             │
                                                   ┌─────────┴──────────┐
                                                   │                    │
                                       [Docker daemon = root]   [Per-App Linux account]
                                                   │                    │
                                         [Application container] ◄──bind mount──┘
                                                   │
                             its own private network + docker0
                                                   │
                          ◄── reaches only the Applications explicitly connected to it
```

## The Vibe AI assistant, and the outbound boundary it creates

This is the only feature in VibeSSH that sends anything about the user's
infrastructure to a party outside their control, so it is the only one whose
boundary points *outwards*.

```
[Desktop app] ──HTTPS, user-supplied endpoint──►  [Any OpenAI-compatible provider]
      │                                            OpenRouter / OpenAI / self-hosted
      │
      └─ sanitizer runs here, before the request is built
```

**Off by default, and off means off.** `AiConfig::enabled` starts `false` on
a fresh install and after an upgrade. Nothing is collected, no client is
built and no endpoint is resolved until the user turns it on and supplies an
address and a model. Turning it off deletes the stored API key rather than
orphaning it.

**The provider is not trusted.** It is an arbitrary URL the user typed. The
design therefore assumes the endpoint may log everything it receives, may be
compromised, and may return hostile text.

**What can cross, and what cannot.**

| Crosses | Never crosses |
|---|---|
| Application name, blueprint, runtime, status, working directory | SSH passwords and key passphrases (keyring-only, never read) |
| Ports, visibility, resource limits | Private key *contents* and their file *paths* |
| Environment variable **names**, and non-secret values | Any environment value flagged secret, or named like one |
| `runtime_config`, with secret keys and flag-values redacted | Agent credentials, the cloud refresh token, the AI key itself |
| Up to 40 recent log lines, redacted | The Node's agent certificate fingerprint |
| Node address, ports, capabilities, CPU/RAM/disk, firewall backend | `AUDIT_REPORT.md` / `FIX_PLAN.md` (excluded from the doc corpus) |

Redaction is `ai::sanitizer`, and it is the second line of defence rather
than the first: secret environment values already read back empty from the
repository, and private keys are referenced by path rather than content, so
the obvious secrets are structurally absent before it runs. What it catches
is the rest - a password typed into a *plain* variable, a `--requirepass`
inside a rendered command array, a connection string embedded in a log line.
It over-redacts where the two conflict.

**The user can see the payload.** `preview_ai_context` returns the literal
text a turn would send, from the same builder the turn uses, and the panel
shows it on demand. This is `AGENTS.md` §6 applied to an outbound boundary:
a boundary the interface does not show is not a boundary.

**Prompt injection is contained by having nothing to inject into.** Log
lines and Application names collected from a Node are attacker-influenced
text, and they end up in a model's context. The system prompt says plainly
that the context is data rather than instruction, but the real mitigation is
structural: this assistant has no tools, executes no commands, and changes
nothing. The worst outcome of a successful injection is a wrong answer.

**What is deliberately still open.**

- **Conversations are not persisted, and that is load-bearing.** Neither
  side keeps a transcript. If persistence is added it needs its own decision
  about retention, because a conversation about a broken Node *is* a copy of
  that Node's configuration and logs.
- **The user's question is not sanitized.** Only collected context is. Someone
  who pastes a password into the chat box has sent it, and no redaction pass
  can reliably tell a pasted secret from prose about one.
- **No egress allow-list.** Any URL the user configures is contacted. This is
  the point of supporting self-hosted endpoints, but it means a mistyped or
  malicious base URL is a real disclosure channel.

## Root on a Node without the desktop in the loop

Two features run as root on a Node on their own, so they are where a mistake
would not wait for an operator to notice it.

**Schedules** (`services::schedule_service`). Cron runs
`/usr/local/lib/vibessh/schedule-runner` as root from
`/etc/cron.d/vibessh-app-<id>`. Every path involved is root-owned and not
writable by anyone else: the runner is `root:root 0755`, the cron file
`root:root 0644`, past runs are kept in `/var/lib/vibessh/schedules` under a
0700 umask. Nothing a person types reaches a cron line except the five time
fields, which are held to digits and `* / , -` - a cron line cannot be ended,
commented or extended with those - and the runner re-checks its ids and its
one-of-three action before calling `docker`, because a cron file is a text
file root can edit by hand. The schedule's name never leaves the local
database. It re-attaches the console only to a FIFO VibeSSH already made, so
it never leaves a root-owned FIFO the admin could no longer open.

**Migration** (`services::migration_service::stream_directory`). The source
Node's `tar` output is extracted as root on the target. That makes a
compromised source Node an attacker against the target, and the defence is
GNU tar's defaults, deliberately not overridden: members with a `..`
component are skipped, a leading `/` is stripped, and symlinks are created
only after every regular file, so no member can be written through one. A
Node whose `tar` is not GNU tar (BusyBox, say) does not get those guarantees
and has not been assessed.

## Attackers, and what they can currently do

| Attacker | Can they cross? | Notes |
|---|---|---|
| Malicious/compromised Application container | **No, for the isolation VibeSSH claims** | Files (per-Application staging, 0600), console (FIFO outside the bind mount) and now network: each Application has its own Docker network and reaches another only where an operator granted it, via a private two-member network. It can still reach the host's MariaDB across `docker0` and is still confined only by that database's own grants — see the open question below. |
| Unprivileged local user on a Node | **No** | Node-side files live under a root-write-only `/run/vibessh`, and what does go to `/tmp` goes into a `mktemp -d` directory rather than a name anyone could create first. Secrets never travel as command arguments (`/proc/<pid>/cmdline` is world-readable) or in a unit file (`/etc/systemd/system` is not private): `--env-file` and `EnvironmentFile=` point at 0600 files. An earlier version of this row claimed the predictable-`/tmp` problem was already gone while the Docker installer still wrote to a fixed path - see the note below on what this table is for. |
| Compromised Node | **No** | Peer values are shape-validated and the generated scripts have no shell-expansion context at all. |
| Remote unauthenticated attacker | **No, for VibeSSH-managed ports** | Non-public ports bind the mesh address, so the kernel refuses them; `DOCKER-USER` rules add filter-level defence. A port published out of band is still the operator's own business. |
| On-path network attacker | **No, after the first connection** | TOFU pinning on both SSH and Agent transports, which detects a key that changes and cannot detect one that was wrong to begin with. Somebody positioned on the path during the very first connection to a Node is trusted at that moment and pinned thereafter. Closing that needs a fingerprint checked out of band, which the interface shows and does not require. |
| Malicious archive | **No** | Zip-slip guarded, declared sizes capped, symlinks skipped. |
| Malicious operator input → SQL | **No** | Backslash-aware escaping; generated identifiers are alphanumeric by construction. |

## Standing assumptions

These are load-bearing and mostly undocumented elsewhere. Breaking one
breaks the model.

1. **The connecting SSH account has passwordless `sudo`.** Docker, ufw,
   WireGuard, `useradd`, `install` and the file helper all depend on it.
   A Node without it fails confusingly rather than being refused.
2. **The admin account is fully trusted.** It is not a boundary. Everything
   VibeSSH does on a Node runs with that account's authority; the
   per-Application accounts exist to separate Applications *from each
   other*, not from the admin.
3. **The desktop machine is trusted.** Keyring contents, and therefore every
   Node, are as safe as the operator's own workstation.
4. **`/run` is `tmpfs`.** Nothing VibeSSH stages there is expected to
   survive a reboot, and nothing should be written there that needs to.

## What this table is for

It records what has been checked, and it is only worth having if a row goes
back to **Yes** the moment something is found. One row here claimed a class
of local-user attack was closed while an installer was still downloading to a
fixed path in `/tmp`; the claim was written when the other instances were
fixed and was never revisited for that one. A row that is aspirational is
worse than a missing row, because it stops the next person looking.

## Open questions

- **The host's database port across `docker0`.** Closing S-018 removed
  Application-to-Application reachability, but every container still reaches
  the host through `host.docker.internal`, which is what a self-hosted
  MariaDB is reached on. The only thing standing between one Application's
  container and another Application's database is the grant
  (`'user'@'172.%'`) and the password. That is a real credential boundary
  rather than a network one, and narrowing it further means either
  per-Application source CIDRs in the grants or a proxy — neither designed
  yet.
- **Authentication rate limiting is per process.** `/auth/login` and
  `/auth/register` are metered per account and per source address, which
  contains both credential stuffing and the cheap denial of service that an
  unmetered Argon2 endpoint offers. The window lives in the backend process,
  so it resets on restart and is not shared between instances - correct for
  the single instance that runs today, and the piece that has to move first
  if this is ever load-balanced.
- **Release signing** (`security-review.md` finding 5). `install.sh`
  verifies a checksum fetched from the same host as the binary, which
  protects against corruption and not against a compromised release host.
  Needs a real signing pipeline.
- **RUSTSEC-2023-0071 in `rsa`, with no fix available.** The Marvin Attack
  is a timing sidechannel that can recover a private key from an oracle that
  performs PKCS#1 v1.5 *decryption*. VibeSSH pulls `rsa` transitively twice:
  through `russh`, which uses it to sign and verify SSH authentication, and
  through `sqlx-mysql`, which uses it to *encrypt* a password with the
  server's public key when a MySQL connection is not already TLS. Neither is
  the decrypting side, which is where the oracle would have to be - so the
  practical exposure looks low, but "looks low" is a judgement, not a
  measurement, and it is recorded here rather than dismissed.

  The crate has no patched release. CI ignores this one advisory id
  explicitly (`.github/workflows/ci.yml`) rather than lowering the whole
  gate. **What removes the line:** a fixed `rsa` release, or `russh` and
  `sqlx` moving off it. Worth re-checking whenever either is upgraded.

- **Handshake replay** (`security-review.md` finding 4). The bearer
  credential is sent as-is with no challenge-response. Now protected in
  transit by a pinned certificate, so the residual risk is a compromised
  intermediate rather than network capture.
