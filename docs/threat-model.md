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
| Cloud backend JWT secret | `backend/.env` | Minimum length enforced at startup. |

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

## Attackers, and what they can currently do

| Attacker | Can they cross? | Notes |
|---|---|---|
| Malicious/compromised Application container | **No, for the isolation VibeSSH claims** | Files (per-Application staging, 0600), console (FIFO outside the bind mount) and now network: each Application has its own Docker network and reaches another only where an operator granted it, via a private two-member network. It can still reach the host's MariaDB across `docker0` and is still confined only by that database's own grants — see the open question below. |
| Unprivileged local user on a Node | **No** | The predictable-`/tmp` symlink and world-readable key paths are gone; VibeSSH's Node-side files live under a root-write-only `/run/vibessh`. |
| Compromised Node | **No** | Peer values are shape-validated and the generated scripts have no shell-expansion context at all. |
| Remote unauthenticated attacker | **No, for VibeSSH-managed ports** | Non-public ports bind the mesh address, so the kernel refuses them; `DOCKER-USER` rules add filter-level defence. A port published out of band is still the operator's own business. |
| On-path network attacker | **No** | TOFU pinning on both SSH and Agent transports. |
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
- **Release signing** (`security-review.md` finding 5). `install.sh`
  verifies a checksum fetched from the same host as the binary, which
  protects against corruption and not against a compromised release host.
  Needs a real signing pipeline.
- **Handshake replay** (`security-review.md` finding 4). The bearer
  credential is sent as-is with no challenge-response. Now protected in
  transit by a pinned certificate, so the residual risk is a compromised
  intermediate rather than network capture.
