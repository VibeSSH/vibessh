# Security review (Etap K)

> ## ⚠️ Superseded — read `AUDIT_REPORT.md` first
>
> **This document was accurate when written and is now dangerously out of
> date in one specific way.** It concluded that command injection and path
> traversal were "not applicable" because nothing in the codebase executed a
> shell command with any input. That was true of the *agent* at Etap J. It
> has not been true of the desktop since Docker, SFTP, the sudo file helper,
> UFW, WireGuard and DNS landed — all of which shell out constantly.
>
> The full audit (`AUDIT_REPORT.md`) found **four CRITICAL findings in
> exactly the category this document marked N/A**, including a shell
> injection in the WireGuard config writer that gave a single compromised
> Node code execution on every other Node in the mesh.
>
> The individual findings below have been re-checked and annotated with
> their current status. Nothing here has been deleted: this is what was
> known at Etap K, and the gap between it and reality is itself worth
> keeping visible.

Reviewed against the codebase as of the Etap J commit, before any of the
fixes below landed. Severities: LOW / MEDIUM / HIGH / CRITICAL, per the
planning doc. HIGH and CRITICAL findings were fixed as part of this same
pass, not just written down - see "Fixed" under each.

## CRITICAL

### 1. Pairing code and issued credential transmitted in plaintext

The entire WebSocket connection - handshake included - was plain `ws://`.
Anyone who could observe the traffic (shared network, malicious router,
ISP, a colocated tenant on the same VPS host's network) could read the
pairing code and, worse, the durable credential issued right after it: a
bearer secret good for full agent access until the next re-pair.

**Fixed**: the public endpoint is now `wss://` with a self-signed
certificate the agent generates once and persists (`agent/src/tls.rs`).
This defeats *passive* eavesdropping - the realistic everyday threat.
It does **not** defeat an *active* attacker positioned on the very first
connection, who could present their own certificate before the desktop has
anything to compare it to (the pairing code is generated blind, before the
desktop has ever talked to the agent - there's no side channel to seed a
pin ahead of time, the same bootstrap problem SSH has before a host's first
`known_hosts` entry). ~~**Residual risk: MEDIUM**~~ — **now fixed.** The
"concrete next hardening step" named here was never taken, and the audit
found it still outstanding (`AUDIT_REPORT.md` S-010): with no pin on top of
`danger_accept_invalid_certs`, Agent Mode was interceptable on *every*
connection, not just the first — and the very next thing sent over it is the
bearer credential.

Trust-on-first-use pinning now exists, matching what `ssh::client` already
did for SSH host keys: migration 15 adds
`servers.agent_certificate_fingerprint`, the fingerprint is checked *before*
the handshake is sent, and it is only ever written when the column is still
NULL — so an interceptor present for one connection cannot make itself
permanently trusted.

### 2. Pairing control endpoint had no code-level guarantee of staying local

`/internal/pair` has zero authentication of its own - its entire security
model is "loopback-only, so reaching it already implies shell-level trust."
That was previously only a code comment. Nothing stopped
`VIBESSH_AGENT_CONTROL_BIND` from being set to a non-loopback address (a
typo, a copy-paste from the public bind config), which would have silently
exposed unauthenticated pairing control to the network - anyone who could
reach it could mint themselves a valid credential.

**Fixed**: `main.rs` now checks the control listener's bound IP after
binding and refuses to start if it isn't loopback, with a log message
naming exactly what's wrong. Verified for real on the test server: setting
`VIBESSH_AGENT_CONTROL_BIND=0.0.0.0:19999` makes the agent log the refusal
and exit instead of serving.

## HIGH

### 3. Agent credential storage existed but was never called

`storage::credentials::store_agent_credential` (OS keyring, tested for
real against Windows Credential Manager in Etap E) was never invoked from
the actual pairing flow - `issued_credential` was displayed and then
discarded the moment the pairing modal closed. Not itself an active
vulnerability (nothing insecure happens - the credential just isn't
persisted for reuse), but a loose end: a tested, security-relevant function
sitting disconnected from its only caller is exactly the kind of thing that
should get wired up or deleted, not left in between.

**Fixed**: `pairing_commands::spawn_pairing_session` now persists a freshly
issued credential to the OS keyring as soon as it arrives, via
`tokio::task::spawn_blocking` so the keyring call doesn't block the async
task forwarding state to the frontend.

## MEDIUM

### 4. Replay of a captured handshake message

The handshake sends the raw `auth_token` (pairing code or credential) with
no challenge-response/nonce. Anyone who obtains a valid handshake message
verbatim (not just the credential value, the whole message) could replay
it to open a new authenticated session. This is the standard bearer-token
pattern (comparable to how most API keys and session tokens work) and is
now protected in transit by TLS (finding 1) the same way HTTPS protects a
bearer token in an `Authorization` header - the residual risk is a
compromised intermediate (a malicious proxy, a logging layer) rather than
network capture. A proper fix (HMAC challenge-response instead of sending
the secret itself) is a real protocol redesign, not proportionate to
retrofit here. **Not fixed** - documented tradeoff, revisit if a threat
model with a semi-trusted intermediary becomes relevant.

### 5. No signature verification on downloaded releases

`install.sh` checks a SHA-256 checksum but nothing signs the release or the
checksum file - if the *hosting itself* were compromised (not just the
network path, which HTTPS to github.com already protects), an attacker
could serve a matching malicious binary and checksum together. Currently
theoretical: no release has ever been published (private repo, no CI
pipeline). **Not fixed** - there's no signing infrastructure to verify
against yet; tracked as a requirement for whenever a real release pipeline
ships (GPG or sigstore/cosign, verified in `install.sh` before trusting the
checksum), not something to fake now.

### 6. No rate limiting on raw connection attempts

The 10-attempt pairing-code burn limit only covers *guessing a code* on an
established connection. Nothing throttles the number of TCP/TLS/WS
handshake attempts themselves - a flood of connections held open up to
`HANDSHAKE_TIMEOUT_SECS` (10s) each could consume file descriptors/memory.
**Not fixed** - this is the same threat model as SSH itself, which doesn't
reimplement connection-rate limiting inside sshd either; the appropriate
layer is the OS/firewall (fail2ban-equivalent, `ufw`'s own rate-limiting
rules), not something to duplicate inside the agent.

## LOW / not applicable yet

- **Command injection / shell escaping**: ~~N/A~~ — **NO LONGER TRUE.**
  This was correct for the *agent*, which still shells out nowhere. The
  desktop now does, everywhere: `runtime::docker`, `files::sudo_user`,
  `firewall::ufw`, `network::wireguard`, `services::dns_service` and
  `services::database_service` all build shell command strings over SSH.

  The advice this entry gave — "start from `Command::new(program).arg(...)`
  with explicit argument arrays, never a shell string built from untrusted
  input" — was right, and was not followed, because SSH exec has no argv
  form to reach for. What happened instead was thirteen copies of a
  `shell_quote` helper, each call site independently deciding what to
  validate, and four CRITICAL findings that were all the same mistake in
  different places (`AUDIT_REPORT.md` S-002 through S-005).

  Now: one `ssh::command` module is the only place a value becomes part of a
  remote command, with `quote` for arguments,
  `reject_shell_metacharacters` for the contexts where quoting is not
  available, and typed validators (`validate_host`,
  `validate_wireguard_key`, `validate_application_directory`, …) for values
  with a known shape. **Anything that builds a remote command goes through
  it.**
- **Path traversal**: ~~N/A~~ — **superseded, and the outcome was good.**
  Real file access exists now (`files::sftp`, `files::local`,
  `files::sudo_user`). Traversal itself is correctly defended:
  `files::sandbox` rejects `..`, absolute paths, backslashes and null bytes,
  and each provider re-checks that the *canonicalised* path is still inside
  the Application's root, so a planted symlink cannot widen it either. Zip
  extraction is guarded by `enclosed_name()` on top of that. This is one of
  the areas the audit verified as genuinely correct.

  What the file layer got wrong instead was not traversal but *exposure*:
  staged copies written world-readable into `/tmp` and never cleaned up
  (S-005).
- **Agent running as root**: it doesn't - runs as the dedicated `vibessh-agent`
  system user, confirmed via `ps -o user` on the test server (Etap G).
- **Downgrade attack**: the protocol version check is strict equality, not
  a minimum-version negotiation - there's no "accept an older dialect"
  path to force. Well-handled by construction, not an add-on.
- **Log leakage**: grepped every `log::*!` call referencing
  credential/code/token - none interpolate the actual secret value, only
  static messages ("pairing code registered", "already paired", etc.).
  Frontend has zero `console.*` calls touching credential/pairing-code
  values either.
- **Secret storage** (the rest of it): agent persists only a SHA-256 hash
  of the credential, compared with a constant-time equality check, never
  the raw value (Etap E, unchanged by this review). Desktop stores the raw
  value only in the OS credential store, verified via a real Windows
  Credential Manager round-trip test. Pairing codes never touch disk at
  all (in-memory `PairingRegistry` only).
- **WebSocket authentication**: every handshake without a valid credential
  or pending pairing code is rejected outright - verified by two dedicated
  tests. No anonymous/unauthenticated access path exists.
- **Local privilege escalation**: covered extensively in
  `docs/agent-privileges.md` (Etap G) - systemd unit management is
  polkit-gated behind a root-owned allowlist the agent cannot self-modify
  (verified for real: empty allowlist denies, adding a unit allows it, the
  agent can't write or delete-and-replace the allowlist file). Docker and
  cross-user process/file access are deliberately not granted to anything
  yet, since no feature uses them.

## A side effect worth calling out explicitly

Fixing finding 1 changed the agent's default bind address from
`127.0.0.1:7420` to `0.0.0.0:7420` - loopback-only was a stopgap for when
there was no transport encryption to make wider exposure safe; TLS plus
the auth that's been enforced since Etap E is the real protection now, and
the whole point of Agent Mode is a desktop reaching a *remote* server, which
a loopback-only default can't do without a manual SSH tunnel (as this
review's own verification needed). Checked before deploying: the project's
test server runs `ufw` with a default-deny incoming policy and no rule for
port 7420, so this change did not, in practice, expose anything there - a
firewall allow rule is still a separate, explicit step for an admin who
wants an agent actually reachable from the internet. That requirement is
documented in `agent-install/README.md`, not automated by `install.sh` -
opening a firewall port is exactly the kind of thing an installer
shouldn't do silently.
