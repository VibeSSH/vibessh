---
id: servers
title: Servers
section: nodes
route: /servers
order: 1
---

A server - also called a Node - is a machine VibeSSH manages. Everything else here (applications, files, the terminal, monitoring) happens on one of them, so adding a server is the first thing you do.

## Two connection modes

**Connect over SSH** - VibeSSH connects the way a person would: over SSH, with your credentials. Nothing is installed on the server. This is the default and the best supported mode.

**Install the Vibe Agent** - a small service runs on the server and the app talks to it over its own protocol. Useful where SSH is awkward. A Node in Agent mode reports no CPU or memory metrics, only a sync state, and has no terminal from this app.

## The SSH connection fields

| Field | Meaning | Example |
| --- | --- | --- |
| Name | The label in the app. Change it freely. | `Production server` |
| Host | An IP address or a hostname. | `203.0.113.10` |
| Port | The SSH port. | `22` |
| Username | The account on the server. | `root` |
| Authentication | A password or an SSH key. | `SSH key` |
| Key path | The private key file on this computer. | `C:\Users\you\.ssh\id_ed25519` |

**Passwords and key passphrases go into the operating system's credential store**, never into a plain configuration file. When editing an existing server, an empty password field means "keep the current one", not "clear it".

## Test connection

Opens a real SSH connection and closes it immediately. **It saves nothing** - you can test, correct and test again before anything reaches the list.

Use it on a first add: a failure shows up straight away with a specific reason, rather than surfacing later during your first file operation.

## The host key

On the first connection VibeSSH remembers the server's public key. If the key does not match next time, the connection is **refused with an error** rather than quietly accepted.

Usually that means the server was rebuilt or reinstalled. But it is the same signal you would get if something were impersonating the server, so VibeSSH does not guess which case it is - that decision is yours.

## Privileges

Many operations on a Node need `sudo`: installing Docker, firewall rules, creating the dedicated accounts applications use, WireGuard. An account without `sudo` will do some of it but not all, and the error will say plainly that sudo refused.

## Common mistakes

- **The test passes but operations fail** - usually no `sudo` for that account.
- **A sudden "host key mismatch"** - take it seriously before deleting the entry. If you rebuilt the machine, all is well; if you did not, it is worth finding out why.
- **I changed the password and it stopped working** - the password is held in the system store per server. After changing it on the server, update it here too.
