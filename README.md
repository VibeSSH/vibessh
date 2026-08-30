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
      **Connect with SSH** now saves for real (Etap 2, below) - add, edit and
      remove all round-trip through the SQLite repository. Paired agent
      servers still only show for the current session, since agent-mode rows
      aren't persisted yet
- [x] Server storage (Etap 2) — SQLite (`rusqlite`, bundled) holds the
      non-secret server row (name/host/port/username/auth type/private key
      *path*); password and key passphrase go to the OS credential store via
      the same `keyring`-backed module pairing's credential uses, now
      generalized to multiple secret kinds keyed by `(server_id, kind)` so
      they never collide. Private keys are referenced by file path, not
      content - Windows Credential Manager caps a generic credential at
      ~2.5KB, too small for a typical key. `server_service` validates input
      and keeps the two stores in sync (delete removes both the row and any
      secrets; a blank password/passphrase on edit means "keep the existing
      one," since the frontend never has it to resend). Covered by 15 Rust
      tests exercising the real SQLite file and the real OS keyring (create/
      update/delete/list, validation failures, secret collision, keep-on-
      blank-update). The Servers page loads/creates/edits/deletes through
      this for real; verified in-browser up to the point a plain browser tab
      can reach (form validation, tab switching, error surfacing all work) -
      the actual Tauri `invoke()` round trip needs the native webview, which
      isn't exercised by that pass
- [x] SSH transport (Etap 3) — `russh` (pure Rust, async/tokio-native, `ring`
      crypto backend rather than the default `aws-lc-rs`, since `ring` is
      the backend already proven to build here for rustls and `aws-lc-rs`
      needs cmake). `ssh::client` connects, authenticates (password or
      private key + optional passphrase, resolved from Etap 2's storage),
      and runs a command over a real exec channel, splitting stdout/stderr/
      exit status. Host key verification is real Trust-On-First-Use, done
      properly from the start rather than deferred like the agent's TLS
      residual risk (Etap K): the first connection to a given server trusts
      and records the host key's SHA-256 fingerprint in a new
      `ssh_known_hosts` table, and every connection after checks against it
      - a changed key (reinstalled server, or an active MITM) is rejected
      with an explicit error instead of silently trusted, exactly like
      OpenSSH's own known_hosts. A cached connection per server id
      (`SshSessionManager`) avoids re-authenticating on every command, with
      a dead-connection retried once before giving up. "Test connection" in
      the Add/Edit Server form opens a real connection against whatever's
      currently typed and closes it again, no save required. Verified two
      ways: 4 integration tests drive `ssh::connect`/`execute_command`
      against a real local `russh::server` instance (successful auth, wrong
      password rejected, first-connection trust, and a changed-key mismatch
      genuinely rejected, not silently accepted), and a one-off manual run
      against the project's real test server confirmed the reported
      fingerprint matches `ssh-keygen -lf`'s independent calculation exactly
      and a real command executed with correct stdout/exit code.
      `get_metrics`/`list_processes`/`restart_service`/`read_file`/
      `write_file` stay honest `not implemented yet` stubs - those need
      their own remote-side mechanics (SFTP, process manager, systemd) that
      are later stages, not "run a command" alone
- [x] Terminal — an interactive PTY + shell over the same `russh` connection
      as Etap 3, not a separate transport. `SshSession::open_terminal` opens
      a channel, requests a PTY and a shell, then spawns a background task
      that bridges it: remote output is forwarded out through a callback,
      writes/resizes come in through an internal channel, and dropping the
      returned `TerminalHandle` is what actually closes the remote channel
      (no separate close-and-forget-to-call-it method). Each terminal gets
      its own Tauri event pair (`terminal://{id}/output`/`/closed`) instead
      of one shared name, since - unlike pairing - more than one can
      reasonably be open at once. The frontend is real xterm.js (`@xterm/
      xterm` + the fit addon), reached from a terminal icon on each SSH
      server's row; output is written straight into xterm with no parsing
      on the frontend side, so real ANSI colors/cursor movement/etc. all
      work exactly like any other terminal emulator. Verified with an
      integration test against a real local `russh::server` implementing an
      echoing shell (open, write, receive echoed output, resize without
      disrupting the session, clean close on drop), and a one-off manual
      run against the project's real test server: a genuine interactive
      `bash` session, MOTD banner, colored prompt, and a command's output
      all round-tripped correctly
- [x] Terminal tabs — the backend was already built for more than one
      session at once (see the per-terminal event pair above), but the
      frontend only ever opened a single `TerminalView`; the Terminal page
      now has a real tab strip, each tab an independent `open_terminal`
      call/`SshSession` channel. All tabs stay mounted simultaneously and
      are shown/hidden with a plain CSS `display` toggle rather than
      conditional rendering, specifically so a backgrounded shell keeps
      receiving output and its scrollback survives switching away from it,
      instead of disconnecting and reconnecting fresh every time it's
      revisited. A hidden tab's `FitAddon.fit()` is a documented no-op
      against a zero-size container (confirmed by reading its source - it
      bails out when it can't measure a cell size rather than throwing or
      collapsing to 0 cols/rows), so a new background tab opens at xterm's
      normal 80x24 default and gets corrected to the real size via the
      existing `ResizeObserver` the moment it's actually shown, rather than
      erroring on mount. Closing a tab closes only that tab's terminal
      session; the others are unaffected. No frontend test runner exists in
      this project to cover this automatically, and multi-session tab
      switching needs the native window to evaluate by hand, which this
      autonomous session couldn't drive - verified with type-checking and a
      production build only, worth trying by hand before trusting fully
- [x] SFTP (Files module) — `russh-sftp` runs the SFTP subsystem over a
      channel on the same `russh` connection, not a separate transport;
      `SshSession` negotiates it lazily on first use (`SshSession::sftp`)
      and reuses it for every later call. `list_directory`/`read_file`/
      `write_file` fill in `ServerConnection`'s three remaining stubs for
      real, giving `RemoteFileEntry` (name/path/is_dir/is_symlink/size/
      modified) a genuine home. `write_file` deliberately doesn't use
      `SftpSession::write`'s plain semantics (open-for-write only, which
      fails on a path that doesn't exist yet) - it opens with CREATE|
      TRUNCATE|WRITE instead, so saving a brand-new file from the UI just
      works instead of erroring on the first save. The frontend is a
      breadcrumb-navigable directory browser (reached from a folder icon on
      each SSH server's row) with a small text editor for files under 1MB -
      opening a file, editing it, and saving goes through real
      read_file/write_file calls, not a mock. Verified with integration
      tests against a real local `russh::server` running the real
      `russh_sftp` server-side protocol handling, backed by an in-memory
      filesystem (write a new file → read it back → see it in a directory
      listing with the right size; overwrite/truncate correctness; a
      missing file fails cleanly and promptly rather than hanging), and a
      one-off manual run against the project's real test server: wrote a
      file over real SFTP, read it back byte-for-byte, and saw it with the
      correct size and modification time in a real directory listing
- [x] Upload/download (Files module) - `download_file`/`upload_file` on
      `SshSession` stream directly between the remote SFTP file and a local
      one via `tokio::io::copy` (russh-sftp's `File` implements
      `AsyncRead`/`AsyncWrite`), the same create-or-truncate semantics as
      `write_file`. Deliberately not built on `read_file`/`write_file`'s
      `Vec<u8>` - those exist for the small-text-file editor, but a real
      upload/download has no business materializing an entire file in
      memory, let alone shipping its bytes across the Tauri IPC bridge as a
      JSON number array the way the editor's read/write commands do. So the
      new `download_remote_file`/`upload_remote_file` commands take a local
      filesystem path instead of file contents - the frontend gets that
      path from a native save/open dialog (`tauri-plugin-dialog`, new this
      phase) rather than ever touching the bytes itself. The Files page
      gained an Upload button in the breadcrumb row (uploads into whatever
      directory is currently open) and a download icon button on every
      file row. Covered by a new integration test against the real local
      `russh::server` harness (download to a real local temp file, then
      re-upload that same file to a new remote path and read it back over
      SFTP to prove both directions independently) and a one-off manual run
      against the project's real test server moving a ~2MB file end to end
      - seeded it remotely, downloaded it, re-uploaded the downloaded copy
      under a new name, and read that back over SFTP, asserting byte-exact
      equality with the original at every step (not just "no error" - actual
      content compared) - large enough that a bug truncating at a single
      SFTP read/write frame boundary would have failed the assertion
- [x] Process manager / resource monitor (Monitor module) — no agent, no
      `sysinfo` (that crate only reads the *local* machine), so `ssh/
      monitor.rs` reads the same `/proc` files and runs the same `ps`/`df`
      a human would at a shell, over one combined `execute_command` call.
      CPU% and network throughput are deltas between two samples (not a
      single `/proc` read - a lone snapshot can't tell you a *rate*), so
      `SshSession` caches the previous sample and diffs against it, the same
      idea `agent::metrics::MetricsCollector` uses locally via `sysinfo`;
      the very first poll after connecting has no prior sample, so it
      reports 0% CPU / 0 bytes-per-sec rather than a meaningless number.
      `list_processes` runs `ps -eo pid,user,pcpu,rss,comm`. The frontend
      polls both every 5s and reuses Etap J's `MetricsPreview` gauges
      (built for Agent mode's push data) unchanged, plus a process table
      sorted by memory. Covered by 5 unit tests against realistic `/proc`/
      `ps` output (a real kernel's actual field layout, not simplified
      fixtures) and a one-off manual run against the project's real test
      server, cross-checked against `free -b`/`df -B1`/`uptime -p`/`ps -e`
      run independently over a second SSH session: RAM and disk totals
      matched to the exact byte, uptime matched to within seconds, and the
      process count matched within the small margin expected between two
      separate samples of a busy box's process churn
- [x] Historical charts (Monitor module) — the Monitor page now keeps its
      own rolling window of the last 60 poll samples (5 minutes at the
      existing 5s interval) in component state, cleared whenever the viewed
      server changes, and renders CPU%/RAM%/network-in/network-out
      sparklines from it via a new `MetricsHistoryChart` - plain inline SVG
      (a polyline scaled into a fixed viewBox plus a filled area under it),
      not a charting library, since a few dozen points is all this needs.
      Deliberately separate from `MetricsPreview`'s gauges rather than
      folded into them: those also back Agent mode's push-fed live preview,
      which has no local sample buffer to chart from, so this only touches
      the SSH-mode polling page. No dedicated frontend test run here (the
      project has no frontend test runner yet) - verified by type-checking
      and a production build; the actual charts need a real polling session
      to look at, which needs the native window this session couldn't drive
      unattended, so this one specifically is worth a manual look before
      trusting it fully
- [x] Systemd quick actions (Actions module) — unlike the agent's own
      systemd support (Etap G), this needs no polkit rule or unit
      allowlist: an SSH session already runs as whatever user it
      authenticated as, so `systemctl restart <unit>` here has exactly the
      privileges that user would have typing the same command by hand -
      there's no separate elevation mechanism sitting in front of it to
      secure. What *does* need guarding is that a service name reaches a
      remote shell command at all: every verb funnels through one
      `run_systemctl` helper that validates the unit name against
      systemd's own allowed character set and requires a `.service` suffix
      *before* it's ever spliced into a command string, so nothing shaped
      like `nginx; rm -rf /` gets anywhere near a shell. Beyond restart,
      the module now exposes start/stop/enable/disable per unit
      (`enable_service` runs `systemctl enable --now` in one round trip,
      since "enable" alone would leave a unit stopped until next boot -
      not what clicking Enable on a currently-inspected unit implies).
      `list_services` combines `systemctl list-units`+`list-unit-files` in
      one round trip to get active and enabled state together - and builds
      its result from `list-unit-files` (the complete, load-state-
      independent universe of every unit systemd knows about from a file)
      enriched with `list-units`, rather than the other way around. That
      ordering matters: `list-units --all` only shows units systemd
      currently has *loaded in memory*, and a stopped `oneshot`/
      `RemainAfterExit` unit gets garbage-collected out of that list even
      with `--all`, despite its file still being on disk. The original
      implementation iterated from `list-units` and merely enriched with
      enabled-state, so a unit that had just been stopped and disabled
      would silently vanish from what the UI showed - caught by a real
      end-to-end run (see below), not the unit tests, since the original
      test fixtures happened to always keep every sample unit "loaded".
      The frontend lists every service with a filter box and start/stop,
      restart, and enable/disable icon buttons, each behind a confirm
      dialog (destructive verbs - stop, disable - get the danger button
      styling; start/restart/enable don't), reached from each SSH server's
      row. Covered by unit tests (rejects real injection payloads, parses
      a realistic aligned `systemctl` listing correctly - this caught a
      real parsing bug during development, where naive whitespace-
      splitting on individually-aligned columns silently produced zero
      results; a dedicated test also covers a unit real but not currently
      loaded, the exact shape of the list-unit-files-vs-list-units bug) and
      two rounds of manual verification against the project's real test
      server: first the original listing/restart run (178 real service
      units with correct active/enabled state for four known services),
      then a full lifecycle run driving a disposable throwaway unit through
      start → enable → disable → stop and asserting the reported state
      after each step, which is what caught the list-vanishing bug above -
      the fix was verified by rerunning the same lifecycle end to end
      afterward and confirming the unit stayed visible with correct state
      throughout. Cleanup independently re-checked afterward
      (`systemctl list-unit-files` for the throwaway unit) confirms nothing
      already running on that shared box was ever touched and no trace was
      left behind
- [x] Docker quick actions (Actions module) — same reasoning as the systemd
      half: no separate elevation mechanism, `docker restart <container>`
      over SSH runs with exactly the privileges (or `docker` group
      membership) the authenticated user already has. Every verb (start,
      stop, restart, remove) funnels through one `run_docker` helper that
      validates the container name/ID against Docker's own allowed
      character set before it's ever spliced into a shell command, exactly
      like `systemd.rs`'s unit-name validation. `list_containers` uses
      `docker ps -a --format '{{.ID}}|{{.Names}}|{{.Image}}|{{.Status}}|
      {{.State}}'` - a `|` delimiter rather than a template `\t` escape
      (not guaranteed to survive a shell round trip the same way a literal
      character does), safe because neither Docker names nor image
      references can ever contain one. The Actions page's Docker
      containers section shares the same icon-button-behind-a-confirm-
      dialog UI as the systemd section (start/stop toggle, restart,
      remove - remove is styled as destructive, since it deletes the
      container rather than just stopping it). Covered by unit tests
      (injection rejection, parsing a realistic multi-container listing)
      and two rounds of manual verification against the project's real
      test server: first the original listing/restart run (5 real
      Pterodactyl-managed Minecraft server containers with correct image/
      status/running state), then a full lifecycle run - `docker create`
      a disposable throwaway container, then start → assert running,
      stop → assert not running, remove → assert gone from
      `list_containers()` - all through this exact code path. Cleanup
      independently re-checked afterward confirms nothing already running
      on that shared box was touched
- [x] Container logs (Actions module) — `container_logs` runs `docker logs
      --tail N --timestamps <container> 2>&1`, merging stdout/stderr into
      one stream in the order Docker actually wrote them (splitting them
      into `execute_command`'s separate stdout/stderr fields would lose
      that interleaving, exactly what matters when reading a crash loop).
      `tail` is clamped server-side to 5000 lines so a fat-fingered request
      can't turn a click into a slow SSH round trip. A terminal-icon button
      per container row opens `ContainerLogsPanel`, a read-only scrollable
      log view with a Refresh button, reusing the same modal chrome as the
      file editor and the systemd/Docker confirm dialogs. Covered by an
      integration test against the real local `russh::server` mock-exec
      harness (proves the tail gets clamped and the exact `docker logs`
      command gets built correctly, by reading it back out of the mock's
      deterministic echo response) and a one-off manual run against the
      project's real test server: pulled the last 20 lines from a live,
      real Pterodactyl-managed Minecraft proxy container and confirmed the
      output was genuine, current log content (real timestamps, a real
      player connect/disconnect line), not a placeholder
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
- [x] Security review (Etap K) — full writeup in `docs/security-review.md`
      covering all 16 areas the plan calls out, with LOW/MEDIUM/HIGH/CRITICAL
      severities. Two CRITICALs, fixed: (1) the pairing code and issued
      credential were transmitted in plaintext - the public endpoint is now
      `wss://` with a self-signed certificate the agent generates and
      persists (defeats passive eavesdropping; an active MITM on the very
      first connection is a documented residual risk - there's no side
      channel to pin a cert fingerprint ahead of time yet); (2) the pairing
      control endpoint had no code-level guarantee of staying loopback-only
      - the agent now refuses to start if it isn't, verified for real. One
      HIGH, fixed: `store_agent_credential` (OS keyring) existed and was
      tested but was never actually called from the pairing flow - it is
      now. Verified against the project's test server, not just unit tests:
      `openssl s_client` completing a real TLS handshake against the
      deployed agent, and the loopback-enforcement refusal triggering for
      real with a misconfigured bind address. Also documents why the
      agent's default bind changed to `0.0.0.0` as a result (TLS + auth are
      the real protection now, not network placement) and confirms that
      change didn't expose anything on the test server, which firewalls the
      port by default
- [ ] Private host mesh — future, architecture reserved for it, not built yet
- [~] Voltius-matched UI — reworking VibeSSH's frontend to match Voltius's
      (github.com/VoltiusApp/voltius, same AGPL-3.0 license) actual look and
      tooling, not just porting design-token *values* the way the palette
      above originally was. In progress; landed so far:
      - Tailwind CSS v4 (`@tailwindcss/vite`) alongside the existing plain
        CSS custom properties, plus a `--t-*` alias layer over the same
        tokens so Voltius's own component code (`var(--t-bg-card)`,
        `bg-(--t-bg-card)`) ports with its own names intact instead of a
        value-by-value translation pass every time
      - Real lucide icons via `@iconify/react` + `@iconify-json/lucide`
        (Voltius's own combination), replacing hand-drawn SVG path
        approximations - a small hand-picked offline subset
        (`scripts/generate-lucide-subset.mjs`), not the full ~1800-icon
        collection or a runtime fetch. Hit and fixed a real bug: the
        default `@iconify/react` entry's async `<Icon>` throws under Vite's
        dependency pre-bundling when nothing async is ever actually needed
        (every icon here is registered synchronously) - switched to their
        dedicated `/offline` entry, which doesn't have the broken code path
        at all
      - Servers page rebuilt as a card grid matching Voltius's grid-layout
        `HostCard` almost exactly: glass surface (backdrop-blur + their
        ring/elevation/highlight shadow recipe), a status dot that pings
        while online, and the flagship element - a mini terminal preview
        bleeding into the card's own corner radius (traffic-light dots,
        `user@host` in terminal colors, blinking cursor on hover) that
        opens a real terminal session on click. Dropped everything tied to
        features VibeSSH doesn't have (pin, team presence, cloud sync,
        vault move/copy, snippets)
      - Layout adopts Voltius's chrome-frame/chrome-slab window layering:
        one frame color painted once across the app, sidebar sitting
        directly on it with no border of its own, and the content area as
        a lighter "slab" floating on top with a rounded top-left corner and
        an ambient shadow
      - Buttons and the generic `Card` component adopt Voltius's
        ring+elevation depth recipe (a subtle ring shadow, brightness
        shift on hover/active) instead of flat background-color swaps;
        `Card` deliberately stays opaque (no blur) rather than glass, since
        Voltius itself reserves the blurred treatment for grid object
        cards and keeps dense/text-heavy surfaces legible
      - File editor is real `@uiw/react-codemirror` now (same library
        Voltius uses), with syntax highlighting via a theme+HighlightStyle
        built from VibeSSH's own tokens (`cmTheme.ts`) and a language
        picked by file extension/name (`editorLanguage.ts`) covering
        json/yaml/js/ts/py/md/css/html/xml/sql plus shell/properties/
        nginx/dockerfile/toml (the last five have no maintained dedicated
        CodeMirror package, so they come from `@codemirror/legacy-modes`)
      - Terminal picked up the four `@xterm/addon-*` packages Voltius uses
        beyond fit: clickable links, OSC 52 clipboard integration,
        GPU-accelerated rendering (graceful fallback to canvas on a lost
        WebGL context), and a real Ctrl+F find-in-scrollback bar backed by
        the search addon
      - Dashboard was wired to real server data while it was being touched
        for this pass (it had never actually read `useServersStore` -
        hardcoded zero stats and a permanent empty state) and now reuses
        `ServerCard` for a "recent servers" grid identical to the Servers
        page; `onEdit`/`onDelete` are optional on `ServerCardProps` so a
        read-mostly surface like this one can omit them, matching Voltius
        using a simpler card on its own Dashboard than the full `HostCard`
      - Every modal in the app (`AddServerModal`, `DeleteServerDialog`,
        `FileEditorPanel`, `ContainerLogsPanel`, the Actions confirm
        dialog) shares one `.modal-panel` class, now on Voltius's opaque
        `.surface-modal-solid` recipe (no blur - every modal here is
        form/code/log-text-heavy, exactly what their own docs say to keep
        opaque) instead of a flat border
      - Every stage above was verified in the browser as it landed
        (screenshots, and for backend-touching pieces like the editor,
        terminal, monitor, and actions pages, a mocked Tauri backend to
        exercise real data paths) - see each stage's own commit for what
        was specifically checked. A full sweep across every real page
        (Dashboard/Servers/Terminal/Files/Monitor/Actions/Settings) with
        mocked data confirmed the shared `Card`/`Button`/`Icon`/modal
        updates already carry the look through consistently - none of
        those pages needed their own page-specific styling to catch up
      - Not ported, deliberately: Voltius's `NavBar` tab-bar navigation
        pattern and its vault-sidebar object model - VibeSSH keeps its own
        route-based sidebar navigation, since only the *look* was asked
        for here, not a rebuild of the navigation architecture around
        features (vaults, teams) this app doesn't have

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
    servers/                AddServerModal (add/edit), SshServerForm (real, Etap 2/3 -
                            includes "Test connection"), DeleteServerDialog,
                            AgentPairingFlow (real), CapabilityBadges, MetricsPreview,
                            TerminalView (real, xterm.js over the Terminal module's
                            open/write/resize/close commands), FileEditorPanel (real,
                            view/edit files under 1MB over read_remote_file/write_remote_file),
                            ContainerLogsPanel (real, docker logs over get_server_container_logs)
  pages/                    Dashboard, Servers, Settings, Terminal (/terminal/:serverId -
                            tab strip, each tab its own open_terminal session),
                            Files (/files/:serverId - breadcrumb-navigable directory browser,
                            native-dialog upload/download via tauri-plugin-dialog),
                            Monitor (/monitor/:serverId - polls every 5s, reuses MetricsPreview,
                            plus a 5-minute rolling MetricsHistoryChart history for CPU/RAM/network),
                            Actions (/actions/:serverId - systemd services + Docker containers,
                            start/stop/restart/enable-disable/remove, each behind a confirm dialog)
  hooks/
  services/                 Tauri command wrappers (pairingService.ts, serverService.ts,
                            terminalService.ts, filesService.ts, monitorService.ts, actionsService.ts)
  stores/                   Zustand stores (serversStore.ts - SSH-mode rows are Etap 2-persisted, agent-mode rows still session-only)
  types/                    incl. pairing.ts (AgentConnectionState), serverEvent.ts (ServerEvent/ServerMetrics),
                            files.ts (RemoteFileEntry)
  config/                   Navigation/module config

src-tauri/                  Desktop backend (Rust, Tauri)
  src/
    commands/                Tauri command entry points (thin), incl. pairing_commands.rs,
                             server_commands.rs, ssh_commands.rs, terminal_commands.rs,
                             file_commands.rs, monitor_commands.rs, actions_commands.rs
    services/                 Business logic, incl. server_service.rs (validation +
                              repository/keyring orchestration), ssh_service.rs
                              (resolves a Server + keyring secret into ssh::connect's/
                              open_terminal's/list_directory's/get_metrics's input)
    models/                    DTOs shared with the frontend (incl. Server/ServerInput/ConnectionMode)
    errors/                     Shared AppError/AppResult
    state/                       AppState, PairingSession (Etap H), SshSessionManager (Etap 3 -
                                 caches one live connection per server id), TerminalSessionManager
                                 (Terminal module - caches open interactive shells by terminal id)
    transport/                    ServerConnection trait (re-exports DTOs from `protocol`)
    agent_client/                  WebSocket client half of the Agent Mode transport
    ssh/                             client.rs (connect/TOFU/auth/exec/open_terminal, `russh`),
                                     sftp.rs (list_directory/read_file/write_file, `russh-sftp`),
                                     monitor.rs (get_metrics/list_processes over `/proc`+`ps`),
                                     systemd.rs (list_services/restart_service, shell-injection-
                                     safe unit name validation),
                                     docker.rs (list_containers/restart_container, same
                                     validation approach as systemd.rs),
                                     transport.rs (adapts SshSession to ServerConnection) - Etap 3
    storage/                           credentials.rs (OS keyring, multiple secret kinds per
                                       server id); server_repository.rs (SQLite, Etap 2 servers +
                                       Etap 3 ssh_known_hosts)
  tests/                       agent_client.rs, ssh_client.rs, ssh_terminal.rs, ssh_files.rs (all
                               drive real protocol code against a local mock server, not a
                               reimplementation of it)
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
    tls.rs                             Self-signed cert generation/persistence (Etap K)
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
