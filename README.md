<div align="center">

# VibeSSH

**Your Linux servers, in one desktop app.**

Terminal, files, monitoring, Docker applications, a private network between
machines and a firewall — without a web panel and without installing anything
on the server to get started.

[![License: AGPL v3](https://img.shields.io/badge/license-AGPL--3.0--or--later-blue.svg)](LICENSE.txt)
[![Download](https://img.shields.io/badge/download-latest%20release-1f9e8c.svg)](https://github.com/VibeSSH/vibessh-releases/releases/latest)
[![Docs](https://img.shields.io/badge/docs-vibessh.dev-1f9e8c.svg)](https://vibessh.dev)

[English](README.md) · [Polski](README.pl.md)

![The VibeSSH dashboard: two nodes online with live CPU and memory, and the applications running on them](shared/guide/images/dashboard.en.png)

</div>

---

## What it is

VibeSSH is a desktop application (Windows and Linux) for people who keep a
few Linux servers and would rather not hold four different tools open to
manage them. It does what Termius, WinSCP, `htop` and a small hosting panel
each do a part of.

It connects in one of two modes, and you can change your mind later:

- **SSH mode** — plain SSH and SFTP. Nothing is installed on the server. If
  you can already `ssh` into a machine, VibeSSH can manage it.
- **Agent mode** — an optional `vibe-agent` daemon on the server adds live
  metrics, streamed logs and a fuller terminal.

The interface never knows which mode it is talking to; both sit behind the
same interface on the Rust side.

## What it does

| | |
| --- | --- |
| **Terminal** | Multiple interactive SSH sessions in tabs |
| **Files** | An SFTP file manager, with an editor and upload/download |
| **Monitor** | CPU, memory, disk and network, processes, systemd units, Docker containers, open ports |
| **Actions** | Start, stop, restart, enable and disable units and containers in one click |
| **Applications** | Docker workloads from a blueprint, with their own files, console, logs, ports, environment, databases and backups — including Minecraft (Paper, Purpur, Velocity, Waterfall) |
| **Vibe Network** | A WireGuard mesh between your servers, with private DNS, so they reach each other without going over the public internet |
| **Vibe Firewall** | Rules derived from what you actually published, with SSH always kept reachable |
| **Accounts and teams** | Optional: shared servers, roles and permissions, and an audit trail |

## Getting started

Download the installer for your system from
[the latest release](https://github.com/VibeSSH/vibessh-releases/releases/latest),
add a server, and the app will offer to set up whatever that server is
missing.

Plain SSH, the terminal and the file manager work on any Linux machine you
can already `ssh` into. Applications, Vibe Network and Vibe Firewall need
three things on the server itself:

- [Docker](https://docs.docker.com/engine/install/) — runs each application as a container
- [WireGuard](https://www.wireguard.com/install/) — the private mesh between servers
- [ufw](https://help.ubuntu.com/community/UFW) — what the firewall is built on

You do not have to install any of them by hand. The Setup page — the gear
icon on a server's card, opened by itself right after you add a server —
detects what is missing and offers to install it over the same SSH
connection. From there it also offers to pair a Vibe Agent and to switch the
firewall on. You always see the exact set of rules before anything is applied.

There is a step-by-step guide inside the app, and the same guide at
[vibessh.dev](https://vibessh.dev).

## Building from source

You need [Bun](https://bun.com) 1.2+ and a stable [Rust](https://rustup.rs)
toolchain. On Windows you also need the
[WebView2](https://developer.microsoft.com/microsoft-edge/webview2/) runtime
(already present on current Windows 10 and 11) and the MSVC build tools
(`winget install Microsoft.VisualStudio.2022.BuildTools`, with the C++
workload) — Rust needs those, not this project in particular.

```bash
bun install
bun run tauri dev
```

`./scripts/setup.ps1` does the same with a prerequisite check first;
`-Dev` runs the app and `-Build` produces the Windows installer.

To run the agent on its own, without the desktop app:

```bash
cargo run -p vibe-agent                          # start it, Ctrl+C to stop
cargo run -p vibe-agent -- pair VIBE-XXXX-XXXX   # in another terminal, once you have a code
```

Normally you do not need those: the Servers page generates and uses pairing
codes for you.

## How the repository is laid out

```
apps/desktop/     the desktop app - ui/ is React, src-tauri/ is Rust
apps/agent/       the optional server-side daemon
apps/backend/     the account, team and permission service
crates/protocol/  the wire format the desktop and the agent share
shared/guide/     the in-app guide - a build input, not documentation about the build
docs/             architecture, security and planning notes
scripts/          setup and maintenance scripts
```

[`docs/repository-structure.md`](docs/repository-structure.md) explains why it
is arranged this way and what belongs where.

## Contributing

Issues and pull requests are welcome. Worth knowing before you start:

- `bun run test`, `cargo test --workspace` and
  `cargo clippy --workspace --all-targets -- -D warnings` are what CI runs;
  running them first saves a round trip.
- Everything a user can read has to exist in both English and Polish. There
  are tests that fail when one is missing.
- Commit messages here explain *why* a change was made rather than restating
  what it changed.

## Security

If you find a vulnerability, please report it privately through
[GitHub's advisory form](https://github.com/VibeSSH/vibessh/security/advisories/new)
rather than opening a public issue.

## Licence

[AGPL-3.0-or-later](LICENSE.txt).
