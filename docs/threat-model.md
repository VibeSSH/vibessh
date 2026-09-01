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
                                       shared vibessh-net + docker0
                                                   │
                                   ◄── reaches every other Application on the Node
```

## Attackers, and what they can currently do

| Attacker | Can they cross? | Notes |
|---|---|---|
| Malicious/compromised Application container | **Partly** | File and console isolation are enforced (staging is per-Application 0600; the console FIFO is outside the bind mount). **Network isolation is not** — every Application shares `vibessh-net` and can reach any other's internal ports by alias. See the open question below. |
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

- **The shared Docker network** (`AUDIT_REPORT.md` S-018). Every
  Application joins `vibessh-net` with a resolvable alias, so Application A
  can reach Application B's *unpublished* ports. This is deliberate — it is
  what makes a Velocity proxy find its Paper backend — but it is not
  currently presented to the operator as a trust boundary at all. Either
  per-Application networks with explicit links, or say so in the UI.
- **Release signing** (`security-review.md` finding 5). `install.sh`
  verifies a checksum fetched from the same host as the binary, which
  protects against corruption and not against a compromised release host.
  Needs a real signing pipeline.
- **Handshake replay** (`security-review.md` finding 4). The bearer
  credential is sent as-is with no challenge-response. Now protected in
  transit by a pinned certificate, so the residual risk is a compromised
  intermediate rather than network capture.
