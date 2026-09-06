---
id: app-waterfall
title: Waterfall
section: blueprints
route: /applications
order: 130
---

Waterfall is a **proxy** based on BungeeCord - one address for players, several Minecraft servers behind it.

It does the same job as **Velocity**. Choose Waterfall when you have plugins written for BungeeCord that you do not want to replace. For a new network, Velocity is the better choice: it is faster and actively developed.

## Step by step

1. Create the servers that will sit behind the proxy first (usually **Paper**), without a public port.
2. **Applications - New application**, the same location as those servers.
3. Pick **Waterfall**, name it e.g. `proxy`.
4. **Waterfall version** and **Java version** - leave the defaults.
5. Create it and start it once, so `config.yml` is written.

## Connecting the servers

1. **Ports** tab - **Connections** card - connect the proxy to every server.
2. **Files** - `config.yml`, the `servers` section, addresses are **application names in lower case** - example below the list.
3. In `config.yml`, set `ip_forward: true`.
4. On every server behind the proxy: `server.properties` - `online-mode=false`, and in `spigot.yml` - `settings.bungeecord: true`.
5. Restart the proxy and the servers.

The `servers` section looks like this:

```
servers:
  lobby:
    address: lobby:25565
    restricted: false
```

## Ports

Only the proxy's port is public. Leave the servers behind it unpublished, or on **Vibe Network**.

## Common problems

**A player joins and is immediately kicked.** Usually `ip_forward` missing on the proxy, or `bungeecord: true` missing on the server.

**The proxy cannot reach a server.** Check the **Connections** card and the address in `config.yml`.
