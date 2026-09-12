<!-- Moved out of the README, which is now a page for people who want to use
VibeSSH rather than build it. Nothing here changed in the move. -->

# Packaging and installers

`bun run tauri build` (or `./scripts/setup.ps1 -Build`) produces a real
Windows installer wizard via NSIS — not a custom-built one, Tauri generates
it from `apps/desktop/src-tauri/tauri.conf.json`'s `bundle.windows.nsis` config:

1. Language selector (Polish / English)
2. Welcome page (branded with `apps/desktop/installer/sidebar.bmp`)
3. License page (`LICENSE.txt` — full AGPL-3.0-or-later text)
4. Install scope: just me / all users (`installMode: "both"`)
5. Install directory
6. Installing... progress
7. Finish

Output lands in `target/release/bundle/nsis/*.exe` (workspace-root target
dir, since `agent/` made this a Cargo workspace). To restyle it, edit the
`nsis` block in `tauri.conf.json` or swap `apps/desktop/installer/header.bmp` and
`apps/desktop/installer/sidebar.bmp` (keep the exact pixel sizes — NSIS requires them).
For anything the config can't express, Tauri supports a fully custom `.nsi`
template via `nsis.template`.

## The agent installer

The *server-side* counterpart to the wizard above: `apps/agent/install/install.sh`
installs `vibe-agent` on a Linux host as a systemd service, run as:

```sh
curl -fsSL <release-url>/install.sh | sudo sh
vibe-agent pair <CODE-SHOWN-IN-VIBESSH>
```

No release is published yet, so the real one-liner can't be exercised
end-to-end - see `apps/agent/install/README.md` for what's actually verified
(architecture/OS detection, checksum logic for real, everything else on a
real Linux box via `VIBESSH_INSTALL_LOCAL_BINARY`) versus what still needs
a Linux VM to prove.

## Icons

`apps/desktop/src-tauri/icons/` currently holds a placeholder generated from
`public/vibessh-mark.svg`. Drop the real VibeSSH mark in as a single square
PNG (ideally 1024x1024) and regenerate the full set with the Tauri CLI:

```bash
bun run tauri icon path/to/vibessh-icon.png
```

This overwrites `apps/desktop/src-tauri/icons/*` with correctly sized PNG/ICO/ICNS files.
Update `public/vibessh-mark.svg` (used in the sidebar) separately if you
swap the mark.
