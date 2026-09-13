# Sharing a Node and its Applications with a team

## What happens today, and why it is half a feature

Adding somebody to a team gives them a **list**. `team_servers` holds a
server's name, host, SSH port and username, and deliberately holds no
password, key path or passphrase - the migration says so in its own comment,
and that was the right call for the stage it was written in.

The result is that a colleague sees the server and cannot touch it. There is
no credential on their machine for that host, so every action fails at the
SSH connection. Applications are worse: they are not shared at all. They live
only in the local SQLite of the install that created them, and the backend
has no table for them.

The permissions catalog has twenty-four entries, fourteen of which name
operations on a Node - `applications.lifecycle`, `node.firewall` and so on.
`permissions.rs` is honest that this backend cannot enforce any of them,
because those operations never reach it: the desktop runs them over its own
SSH connection with the operator's own credentials. They hide buttons. They
are not a boundary.

So a team today is a shared address book with advisory labels.

## What this is meant to become

A member who has been added to a team, and given a role, can open the shared
Node and work with the Applications on it - within what their role allows,
enforced rather than suggested.

## The decision: a separate SSH account per member

Each member gets their own Linux account on the Node. Their own machine
generates a keypair; the **public** half is installed on the Node by an
install that already has access. No private key and no password ever leaves
the machine it was generated on, and nothing of the sort passes through the
backend - which keeps `0006_team_servers.sql`'s promise intact rather than
reversing it.

The alternatives were considered and rejected:

- **Sharing the owner's credential, encrypted end-to-end to each member.**
  The backend would hold only ciphertext, which is a real improvement over
  holding the secret - but every member then *is* the owner on that machine,
  nothing distinguishes their actions in the Node's own logs, and revoking
  one member means rotating a key on every Node and re-encrypting for
  everyone who remains.
- **The backend holding credentials.** One breach of one service would hand
  over root on every user's servers. The whole product is premised on the
  opposite.

### What this buys, stated precisely

**Accountability.** Every action on the Node runs as a named account, so the
Node's own auth log and `sudo` log attribute it to a person. Today everything
is the owner, whoever actually did it.

**Revocation that is one step and is local.** Removing a member removes their
key from that Node. Nothing has to be rotated, and nobody else is disturbed.

**Not, by itself, less privilege.** This is the part that must not be
oversold. The desktop's Node operations use `sudo` for `docker`, `ufw`,
`iptables`, `wg`, `install`, `chown`, `rm`, `apt-get` and `systemctl`. An
account that can do everything the app does is root in all but name. A
separate account alone gives accountability and revocability - it does not
reduce what a member could do if they went around the app.

## Making the permissions real

That last point is also the opportunity. A member's account does not need all
of that `sudo`; it needs what their role allows. The catalog already names
the operations, so the sudoers file for a member can be **derived from their
role** rather than being one blanket rule.

The mapping is bounded for the permissions people actually hand out:

| Permission | What the account needs |
| --- | --- |
| `applications.lifecycle` | `docker start|stop|restart|kill vibessh-app-*`, `systemctl start|stop|restart vibessh-app-*.service` |
| `applications.view` | `docker ps`, `docker inspect vibessh-app-*`, `systemctl status vibessh-app-*` |
| `applications.files.read` / `.write` | Read/write under the Application's working directory, via its dedicated account - the mechanism `files::sudo_user` already uses |
| `node.firewall` | `ufw`, and `iptables` limited to the `DOCKER-USER` chain |
| `node.network` | `wg`, `wg-quick` |
| `node.software` | `apt-get`, the installer scripts - effectively root, and should be said to be |
| `node.terminal` | A login shell, which is root-equivalent if the account has any broad sudo at all |

Two of those rows are honest admissions rather than restrictions:
`node.software` and `node.terminal` cannot be meaningfully narrowed. A role
carrying either should say so where it is granted, in the same place the
interface already explains that permissions are guard rails.

For the rest, this turns the catalog into something the Node enforces. That
is a bigger change in what VibeSSH means by "role" than the sharing feature
itself, and it is the reason to do this rather than the easier option.

## Applications in a team

Applications gain a team-scoped record, the same shape `team_servers` has: id,
team, node, name, blueprint, runtime type, working directory, ports and
non-secret environment.

**Secret environment values do not go.** They are already on the Node, in the
Application's own environment file, which is where the process reads them
from. A member does not need them to start, stop, inspect or back up an
Application - only to recreate one from scratch, which is a separate
permission (`applications.create`) and can ask for them at that moment. This
is not a compromise to work around the design; it is the design paying off.

Two sources of truth are the risk here. The local SQLite stays authoritative
for the install that owns the Application, and the team record is a
projection of it, refreshed on change. A member's install reads the team
record and reconciles against the Node - the same "check, do not assume"
stance the firewall overview now takes after the audit.

## Revocation

Removing a member from a team must remove their account's key from every Node
that team shares. That runs from an install with access, which means it can
fail while the member still has their key.

So the record of what should be revoked lives in the backend, and any
install with access to that Node completes it - the same reconcile shape
`firewall_service` uses. A revocation that has not landed yet is shown as
pending rather than reported as done, because the difference between "this
person no longer has access" and "we asked" is the entire point.

## Open questions

- **Who provisions.** The flow needs an install with access to the Node at
  the moment a member is added. If the owner is offline, the member's access
  is pending. Acceptable, but it has to be visible.
- **The agent as an alternative path.** A Node running the Vibe Agent could
  provision and revoke without the owner's machine being involved, which
  would remove the pending state. That is a larger build and should not hold
  up the first version.
- **Key rotation for a member** across many Nodes - the same reconcile, but
  worth designing once rather than per feature.
- **Where the member's keypair lives.** The OS keyring, like every other
  secret the desktop holds. A member with two machines gets two keys, which
  is correct rather than inconvenient: revoking one laptop should not revoke
  the other.

## Stages

1. **Per-member accounts and key installation**, with a single blanket
   sudoers rule and the privilege position stated plainly in the interface.
   This is the point at which a colleague can actually do something.
2. **Applications shared to the team**, non-secret fields, reconciled against
   the Node.
3. **Role-derived sudoers**, which is where permissions stop being advisory.
4. **Revocation reconcile**, including the pending state.

Stage 1 without stage 3 must not describe roles as restrictions anywhere in
the interface. That wording can only appear once the Node enforces them.
