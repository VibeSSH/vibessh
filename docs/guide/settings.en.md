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
