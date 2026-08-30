<div align="center">

# VibeSSH

**A hybrid SSH/SFTP desktop client for Linux servers.**
Fast in SSH mode, no install required — superpowered with an optional Rust agent.

[![License: AGPL v3](https://img.shields.io/badge/license-AGPL--3.0--or--later-blue.svg)](LICENSE.txt)
![Status](https://img.shields.io/badge/status-early%20development-orange)

</div>

---

VibeSSH is a desktop app (Tauri + React + TypeScript + Rust) that plays the
role of Termius + WinSCP + htop + a lightweight Pterodactyl-style panel, but
as a native desktop client instead of a web panel.

It connects to your servers in one of two modes:

- **SSH Mode** — plain SSH/SFTP. Nothing to install on the server, works
  with any box you can already `ssh` into.
- **Agent Mode** — pair an optional `vibe-agent` daemon on the server for
  realtime metrics, logs, and a more capable terminal, without giving up
  SSH Mode's simplicity for servers where you'd rather not install anything.

The frontend never knows which mode it's talking to — both go through the
same `ServerConnection` interface on the Rust side.

## Modules

- **VibeSSH Terminal** — interactive, multi-session SSH terminal
- **VibeSSH Files** — SFTP file manager
- **VibeSSH Monitor** — dashboard, process manager, systemd, Docker, ports
- **VibeSSH Actions** — scripted multi-step quick actions
- **VibeSSH Pro** — advanced features (incl. a Minecraft server module)

## Roadmap

- [x] App shell — layout, routing, design system, reusable components
- [x] Connection model + transport abstraction (`ServerConnection` trait)
- [x] Vibe Agent skeleton — standalone daemon, durable identity, no network yet
- [x] Desktop ↔ Agent protocol — WebSocket, handshake/version check, heartbeat,
      reconnect with backoff, typed event enum (`protocol` crate, shared by
      both sides). TLS deferred to the security review, not skipped
- [x] Agent pairing — one-time `VIBE-XXXX-XXXX` code registered locally via
      `vibe-agent pair <code>`, single-use, 5-minute TTL, brute-force budget;
      issues a durable credential whose hash (never the raw value) is the
      only thing persisted on the agent, and which the desktop stores in the
      OS credential store (Windows Credential Manager / Keychain / Secret
      Service), not a plaintext file
- [x] Agent installer — `agent-install/install.sh`: detects OS/arch,
      downloads + checksums a release, installs a dedicated systemd service
      as a non-root user. Run for real end-to-end on a live Ubuntu 24.04 box
      (see `agent-install/README.md`) - only the download step is unverified,
      since there's no published release to fetch yet
- [x] Systemd/privilege hardening (Etap G) — `docs/agent-privileges.md`
      analyzes which future agent features need elevated access and why.
      Systemd unit management (Quick Actions) gets a polkit rule authorizing
      only allowlisted units, with the allowlist file deliberately
      unwritable by the agent itself. Docker and cross-user process/file
      access are documented but intentionally deferred - no feature needs
      them yet, so nothing is granted yet. Verified for real: empty
      allowlist denies, adding a unit allows it, other units stay denied,
      and the agent genuinely cannot rewrite its own allowlist
- [x] Desktop UI for pairing (Etap H, partial) — "Add Server" modal with two
      real tabs: **Install Vibe Agent** actually generates a pairing code,
      starts `agent_client::run` via a Tauri command, and streams connection
      state to the UI over Tauri events (Connecting → Connected, with a
      live TTL countdown and a "generate new code" escape hatch) - this is
      real, not mocked, verified against a real WebSocket handshake.
      **Connect with SSH** is real UI with no backend to submit to yet
      (`SshTransport` is Etap 3). Paired/added servers show in a list with
      type (SSH/Agent) and status badges - session-only for now, since
      server storage (Etap 2) isn't built
- [ ] SSH transport implementation
- [ ] Server storage (SQLite for server records - credential storage already
      landed early, see pairing above; the server *list* is currently
      in-memory only, see Etap H)
- [ ] Terminal, SFTP, process manager, systemd, Docker
- [x] Capabilities (Etap I) — the agent detects real host state on every
      accepted handshake (`systemd` via `/run/systemd/system`, `docker` via
      the socket file, `minecraft` by scanning `/proc` for a Java process
      launching a recognizable server jar) and reports it; a `true` means
      the *host* supports it, not that VibeSSH has that feature built yet.
      The desktop shows capability badges (supported vs. struck-through) in
      the pairing flow and the server list instead of assuming every Linux
      box has Docker/systemd. Verified for real against the project's test
      server - including `minecraft: true`, correctly detecting an actual
      running Minecraft server process there
- [x] Realtime metrics (Etap J) — the agent samples CPU/RAM/disk/load
      average/uptime/network RX-TX (via `sysinfo`, not hand-rolled `/proc`
      parsing) once per connection and pushes `metrics.update` on a 5s
      interval with real backpressure (`MissedTickBehavior::Delay` - a slow
      reader delays the next tick instead of the connection bursting a
      backlog once it catches up). Desktop surfaces this as a live
      `MetricsPreview` fed by the connection the pairing flow already has
      open - real push data, not polled, not mocked, though not yet wired
      into a persistent Dashboard session (that needs Etap 2 storage first).
      Verified end-to-end against the test server through a real WebSocket
      handshake: reported RAM/disk totals matched what `free -h`/`df -h`
      showed on that box independently
- [ ] Security review pass (pairing, TLS, secret storage, privilege escalation)
- [ ] Private host mesh — future, architecture reserved for it, not built yet

Built in stages on purpose — each one lands as something that actually runs
and can be tested, not a partial slice of a bigger unfinished feature.

## Prerequisites

- [Node.js](https://nodejs.org) 18+
- [Rust](https://rustup.rs) (stable toolchain)
- On Windows: [WebView2](https://developer.microsoft.com/microsoft-edge/webview2/) runtime (preinstalled on modern Windows 10/11)
- On Windows: MSVC build tools (`winget install Microsoft.VisualStudio.2022.BuildTools` with the C++ workload) — required by Rust, not specific to this project

## Getting started

```bash
./scripts/setup.ps1        # check prerequisites, npm install
./scripts/setup.ps1 -Dev   # run the desktop app in dev mode
./scripts/setup.ps1 -Build # build the Windows installer
```

Or manually:

```bash
npm install
npm run tauri dev
```

Try the agent on its own (no desktop app needed):

```bash
cargo run -p vibe-agent                          # start the daemon, Ctrl+C to stop
cargo run -p vibe-agent -- pair VIBE-XXXX-XXXX   # in a second terminal, once you have a code
```

The desktop app's Servers page ("Add server" → "Install Vibe Agent") now
generates and uses pairing codes for you - the commands above are for
testing the agent standalone, without the desktop app running.

## Project structure

```
Cargo.toml                  Workspace root (members: src-tauri, agent, protocol)

src/                        Frontend (React + TypeScript)
  components/
    layout/                 Sidebar, Topbar, AppLayout
    ui/                     Reusable design-system components
    servers/                AddServerModal, SshServerForm (placeholder), AgentPairingFlow (real),
                            CapabilityBadges, MetricsPreview
  pages/                    Dashboard, Servers, Settings
  hooks/
  services/                 Tauri command wrappers (incl. pairingService.ts)
  stores/                   Zustand stores (incl. serversStore.ts - session-only until Etap 2)
  types/                    incl. pairing.ts (AgentConnectionState), serverEvent.ts (ServerEvent/ServerMetrics)
  config/                   Navigation/module config

src-tauri/                  Desktop backend (Rust, Tauri)
  src/
    commands/                Tauri command entry points (thin), incl. pairing_commands.rs
    services/                 Business logic
    models/                    DTOs shared with the frontend (incl. Server/ConnectionMode)
    errors/                     Shared AppError/AppResult
    state/                       AppState, PairingSession (Etap H's running-task handle)
    transport/                    ServerConnection trait (re-exports DTOs from `protocol`)
    agent_client/                  WebSocket client half of the Agent Mode transport
    ssh/                             Reserved for SshTransport impl
    storage/                           credentials.rs (OS keyring); server repository still reserved
  icons/                       App icon set (placeholder — see below)

agent/                       Vibe Agent daemon (Rust, Tokio, no Tauri/GUI)
  src/
    main.rs                   CLI entry: `pair <code>` or daemon startup
    cli.rs                     `vibe-agent pair` - blocking call to the control endpoint
    lib.rs                      Library half - what tests/handshake.rs drives
    identity.rs                  Durable UUID, persisted to disk
    info.rs                       AgentInfo DTO (id/version/hostname/os/status)
    config.rs                      Data/config dir resolution
    errors.rs                       AgentError/AgentResult (own type, not shared with desktop)
    capabilities.rs                  detect() - real systemd/docker/minecraft/terminal detection
    metrics.rs                        MetricsCollector - real CPU/RAM/disk/load/uptime/network via sysinfo
    pairing/                          PairingRegistry (one-time code) + credential.rs (hash, never plaintext)
    transport/                         WS server (handshake, heartbeat, metrics tick) + local-only pairing control HTTP route

protocol/                    Shared Desktop<->Agent DTOs (no I/O, no runtime)
  src/
    handshake.rs               HandshakeRequest/Response, PROTOCOL_VERSION
    events.rs                    ServerEvent enum (metrics.update, terminal.output, ...)
    dto.rs                         CommandOutput, ServerMetrics, ProcessSummary, ServiceSummary
    error.rs                        ProtocolErrorCode
    pairing.rs                       generate_pairing_code(), PAIRING_CODE_TTL
    capabilities.rs                   AgentCapabilities (docker/systemd/minecraft/fileAccess/terminal)

scripts/
  setup.ps1                  Setup/build launcher

installer/
  header.bmp                 NSIS wizard header banner (150x57)
  sidebar.bmp                NSIS wizard Welcome/Finish page art (164x314)

agent-install/               Linux agent installer (curl | sudo sh) - not
  install.sh                 the same thing as installer/ above, which is
  test.sh                    the Windows *desktop app* installer wizard
  README.md

LICENSE.txt                  Shown on the installer's license page
```

## Installer wizard

`npm run tauri build` (or `./scripts/setup.ps1 -Build`) produces a real
Windows installer wizard via NSIS — not a custom-built one, Tauri generates
it from `src-tauri/tauri.conf.json`'s `bundle.windows.nsis` config:

1. Language selector (Polish / English)
2. Welcome page (branded with `installer/sidebar.bmp`)
3. License page (`LICENSE.txt` — full AGPL-3.0-or-later text)
4. Install scope: just me / all users (`installMode: "both"`)
5. Install directory
6. Installing... progress
7. Finish

Output lands in `target/release/bundle/nsis/*.exe` (workspace-root target
dir, since `agent/` made this a Cargo workspace). To restyle it, edit the
`nsis` block in `tauri.conf.json` or swap `installer/header.bmp` and
`installer/sidebar.bmp` (keep the exact pixel sizes — NSIS requires them).
For anything the config can't express, Tauri supports a fully custom `.nsi`
template via `nsis.template`.

## Agent installer

The *server-side* counterpart to the wizard above: `agent-install/install.sh`
installs `vibe-agent` on a Linux host as a systemd service, run as:

```sh
curl -fsSL <release-url>/install.sh | sudo sh
vibe-agent pair <CODE-SHOWN-IN-VIBESSH>
```

No release is published yet, so the real one-liner can't be exercised
end-to-end - see `agent-install/README.md` for what's actually verified
(architecture/OS detection, checksum logic for real, everything else on a
real Linux box via `VIBESSH_INSTALL_LOCAL_BINARY`) versus what still needs
a Linux VM to prove.

## Icon

`src-tauri/icons/` currently holds a placeholder generated from
`public/vibessh-mark.svg`. Drop the real VibeSSH mark in as a single square
PNG (ideally 1024x1024) and regenerate the full set with the Tauri CLI:

```bash
npm run tauri icon path/to/vibessh-icon.png
```

This overwrites `src-tauri/icons/*` with correctly sized PNG/ICO/ICNS files.
Update `public/vibessh-mark.svg` (used in the sidebar) separately if you
swap the mark.

## License

VibeSSH is open source, licensed under the [GNU AGPL v3.0 or later](LICENSE.txt).
In short: if you run a modified version as a network service, you must make
that version's source available to its users, and any redistribution stays
under the same license.

UI/UX takes inspiration from other open-source terminal clients (e.g.
[Voltius](https://github.com/VoltiusApp/voltius), also AGPL-3.0) but is
implemented as original code — ideas, not vendored code.
