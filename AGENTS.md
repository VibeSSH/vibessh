# Working on VibeSSH

Rules that exist because breaking them already caused a specific,
identified bug. Each one names it. Nothing here is style preference — those
belong in review comments, not a rules file.

Written for anyone touching this codebase, human or otherwise.

---

## 1. Never build a remote command by hand

**Everything that becomes part of a command sent to a Node goes through
`ssh::command`.**

- `command::quote(value)` for a value that is one argument. POSIX
  single-quoting; the remote shell then treats every byte literally.
- `command::reject_shell_metacharacters(value, field)` for a value landing
  somewhere quoting is not available — a generated config, a heredoc body.
- `command::validate_*` when the value has a known shape. A hostname
  validated as a hostname rejects far more than any character denylist, and
  produces a better error than a remote failure would.

**Why.** `shell_quote` was copy-pasted into thirteen modules, each call site
deciding independently what to validate. Four of the seven CRITICAL findings
in `AUDIT_REPORT.md` were the same mistake in different places. The worst,
S-002, wrote peer data into an *unquoted* heredoc guarded only by a newline
check — and a peer's public key is whatever `wg pubkey` printed on that
peer's own Node, so one compromised Node got code execution on every other
Node in the mesh.

**Prefer a shape with no expansion context at all.** `network::wireguard`
builds its config from single-quoted `printf` arguments rather than a
heredoc: there is then nothing for `$`, a backtick or a backslash to do. A
quoted heredoc is second best; an unquoted one is never acceptable.

## 2. A secret never appears in a command string

Use `ssh::write_private_file`, which creates the file mode 0600 *before*
writing to it and moves the content over SFTP.

**Why.** A command string is visible in `ps` to every local account on the
Node for as long as the command runs. This bit twice: `MYSQL_PWD=...` and
`sudo mysql -e '<sql containing the password>'` (S-008), and separately
`printf '%s' '<password>' | docker login` — which survived the first fix
*and* had a doc comment claiming it did not do that.

If you write a comment asserting a security property, check it against the
line below it.

## 3. `let _ = ...` on anything the user can see is a bug

Either handle the error, log it with `log::warn!`, or return it. Discarding
it silently is the thing that made a whole class of findings possible:
firewall syncs that reported success while enforcing nothing, keyring
secrets that outlived what they belonged to, staging files that accumulated
forever because the cleanup could never have worked.

`credentials::forget_secret` is the pattern for "must not fail the
operation, must not vanish".

## 4. Node-side files go in `/run/vibessh`, never `/tmp`

`/tmp` is world-writable *and* world-readable. A predictable path there,
written through `sudo`, is a symlink attack (S-003 overwrote a root-owned
file); staged content there is a disclosure (S-005 left a world-readable
copy of every file anyone opened, and could never delete them, because
`/tmp`'s sticky bit stops a non-owner unlinking).

`node_paths` has the layout and the reasoning.

## 5. Deleting something means deleting all of it

`delete_application` destroys the container, drops the databases, revokes
the firewall rules, removes the DNS name and removes the Node's account —
and reports what it could not do. It used to delete a database row, leaving
a container running under `--restart unless-stopped`, still holding its
published port (S-007).

If you add a resource created on a Node, add it to the teardown in the same
change.

## 6. A boundary the UI doesn't show isn't a boundary

Every Application used to share one Docker network, so any container could
reach any other's *unpublished* ports. That was a deliberate design choice -
it is what let a Velocity proxy find its Paper backend - and it was written
down nowhere the operator could see. It is now default-deny with an explicit
allow-list (S-018, `runtime::docker`).

If you add something that lets one Application affect another, it needs a
place in the interface where an operator can see it and turn it off. "It's
documented in a doc comment" is not that place.

## 7. Don't infer a caller's behaviour from the callee

Three findings in `AUDIT_REPORT.md` were wrong, all the same way: reading
one function correctly, then reasoning about what the rest of the system
must therefore do, without opening the call sites. In every case the
surrounding code already handled it. See §16 of that report.

Open the call sites.

---

## Verifying a change

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
npm run typecheck && npm test && npm run build
```

CI runs all of these. Warnings are denied, so a new one fails the build —
that is deliberate: 93 had accumulated before the gate existed.

**Tests requiring something real are `#[ignore]`d**, not deleted:
`cargo test -- --ignored` with a live Node, and
`DATABASE_URL=... cargo test -p vibessh-backend -- --ignored` for the
backend. Marking them was necessary because cargo stops at the first
failing test binary, so unignored Postgres tests took the entire workspace
suite down on any machine without a database.

**None of the Node-side behaviour has been verified against a real host.**
Command construction is unit-tested and the generated shell was checked with
`sh -n`, but `mktemp`/`install -d`/`iptables`/`bind-address`, the teardown
sequence, the `/etc/hosts` rewrite and archive streaming all still need an
integration pass. Treat that as the largest outstanding risk.

## Where things are

| Document | What it is |
|---|---|
| `AUDIT_REPORT.md` | The full audit. §16 lists its own corrections — read it before trusting a finding. |
| `FIX_PLAN.md` | Phased remediation, annotated with what is done and what was deliberately skipped. |
| `docs/threat-model.md` | Maintained. Update it when a feature crosses a boundary. |
| `docs/security-review.md` | Historical (Etap K), superseded, annotated where it became false. |
| `docs/agent-privileges.md` | The privilege model, corrected where the decision it described was overtaken. |
| `docs/APPLICATIONS_ARCHITECTURE.md` | The Applications design. Accurate. |
