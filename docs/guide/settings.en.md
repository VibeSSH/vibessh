---
id: settings
title: Settings
section: getting-started
route: /settings
order: 90
---

Settings gathers what applies to the whole app rather than to one Node: the language, access to external services, and the assistant's configuration.

## Preferences

**Language** switches the interface between Polish and English. It is personal and **only on this device** - it syncs nowhere.

The guide follows this setting. A topic not yet written in your language appears in the language it exists in, and says so.

## Backup destination

An S3-compatible store that application backups are copied to. You fill in **Endpoint**, **Region**, **Bucket** and a key pair.

**Path-style addressing** has to be ticked for most MinIO installations - it is not cosmetic; without it the connection simply does not work.

When editing, **an empty Secret Access Key means "keep the current one"**, not "clear it". The key goes into the operating system's credential store.

**Test connection** checks access before the first backup tries to upload. Worth doing - otherwise you find out the configuration was wrong at the moment a backup was supposed to already exist.

> A backup that lives only on the same Node as the application protects you from a mistake, but not from losing the Node.

## Private Docker registries

Credentials for image registries. **Public images work without signing in** - you add a registry only when you need a private image.

One entry per registry, used by every application whose image comes from there. The address is the host: `docker.io`, `ghcr.io`, or your own registry's address. The password or access token goes into the credential store.

## DNS suffix

The ending for names in Vibe Network's private DNS - `.vibe` by default, so a Node is `server.vibe`. Changing it affects every alias, so synchronise Vibe Network afterwards.

## Vibe AI

Turning the assistant on, the provider, base URL, model, API key and a connection test. The shared model's daily usage is shown here too. The Vibe AI topic covers the detail.

**The API key goes into the operating system's credential store and is not returned to the interface after saving.** An empty field when editing means "keep the current one".

## About

The version, and a check that the Rust backend is answering. If it sits on "Waiting for the backend", the interface works but nothing beneath it does - no operation will succeed.

## Common mistakes

- **I cleared the key field to remove it** - an empty field keeps the current key. Removing one is a separate action on that entry.
- **Backups are not reaching S3** - use Test connection and check path-style; with MinIO that is the usual cause.
- **I changed language and the guide is in English** - that topic has no version in the chosen language yet. The page says so.
