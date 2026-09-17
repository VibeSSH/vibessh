---
id: settings
title: Settings
section: getting-started
route: /settings
order: 140
---

Settings gathers the options that apply to the whole app.

## How to change the language

1. Open **Settings**.
2. In the **Preferences** card, choose **Language**.

The setting applies to this device only.

## What the close button does

By default the close button **does not quit VibeSSH** - it hides the window and leaves the
program beside the clock. Open SSH sessions, port forwards and followed logs keep running.
The first time this happens you get a notification, so you are not left hunting for a
program that seems to have vanished.

The icon beside the clock is often tucked under the **^** arrow - click it to expand the list.

1. **Click the icon** - the window comes back.
2. **Right-click the icon** - a menu with three items:
   - **Show VibeSSH** - the same as clicking the icon.
   - **Check for updates...** - opens the window and looks for a newer version.
   - **Quit VibeSSH** - actually closes the program.

## How to make the close button quit instead

1. Find the **Preferences** card, the **Closing the window** row.
2. Turn off **Keep VibeSSH running in the tray**.
3. From then on the close button ends VibeSSH along with everything it was doing -
   including open sessions and port forwards.

The setting is saved straight away, so it survives even a sudden shutdown.

## How to set a backup destination (S3)

1. Find the **Backup destination** card.
2. Tick **Upload backups to external storage**.
3. **Endpoint** - the storage address.
4. **Region**, **Bucket** - from your storage provider.
5. **Access Key ID** and **Secret Access Key** - the access keys.
6. For MinIO, tick **Path-style addressing**.
7. Click **Test connection**.
8. Save.

## How to add a private Docker registry

1. Find the **Private Docker registries** card.
2. Click **Add registry**.
3. **Registry address** - e.g. `ghcr.io`.
4. **Username** and **Password / access token**.
5. Save.

Public images work without signing in.

## How to change the DNS suffix

1. Find the **DNS suffix** card.
2. Enter the new ending, e.g. `vibe`.
3. Save.
4. Open **Vibe Network** and click **Sync Vibe Network**.

## How to check it works

- The interface changes language immediately.
- The storage connection test succeeds.
- The **About** card shows the version and the backend's answer.

## Common problems

- **Backups are not reaching S3** - use **Test connection**. With MinIO the usual cause is **Path-style addressing** being unticked.
- **I cleared the key field to remove it and nothing changed** - an empty field keeps the current key.
- **"Waiting for the backend"** - the interface works but nothing beneath it does. Restart the app.
- **I closed the window and cannot find the program** - you did not close it, you hid it.
  Look for the VibeSSH icon beside the clock; it may be under the **^** arrow. If you would
  rather the close button quit, turn off **Keep VibeSSH running in the tray** in
  **Preferences**.
