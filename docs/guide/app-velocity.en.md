---
id: app-velocity
title: Velocity
section: blueprints
route: /applications
order: 120
---

Velocity is a **proxy** - one address players connect to, with several Minecraft servers behind it that they can move between without disconnecting.

It is not a server itself. No world, no Bukkit plugins, nothing to play on. It routes traffic to servers you create separately.

## When you need one

When you have more than one server and want players to type a single address: lobby, survival, minigames. With one server, Velocity adds nothing.

## Step by step

1. First create the servers that will sit behind the proxy - usually **Paper**. Do not publish port `25565` for them; players are meant to arrive through the proxy.
2. **Applications - New application**, choose **the same location** as those servers.
3. Pick **Velocity**, name it e.g. `proxy`.
4. **Velocity version** - pick from the list.
5. **Java version** - leave `21`.
6. Create it and press **Start** once, so the configuration files are written.

## Connecting the servers to the proxy

1. Open the proxy application, go to the **Ports** tab, find the **Connections** card. Connect the proxy to every server that belongs behind it. Without this the proxy cannot see them.
2. **Files - Quick files - velocity.toml**. In the `[servers]` section list the servers, using **application names in lower case** - example below the list.
3. Copy the contents of `forwarding.secret` (also in Quick files).
4. On every server behind the proxy: `server.properties` - `online-mode=false`, and in `config/paper-global.yml` enable `velocity` and paste the same secret.
5. Restart the proxy and the servers.

The `[servers]` section looks like this:

```
[servers]
lobby = "lobby:25565"
survival = "survival:25565"
try = ["lobby"]
```

Step 4 is not optional. A server behind a proxy with `online-mode=true` will reject players, and without the secret anyone can connect to it directly, bypassing the proxy, under any name they like.

## Ports

The proxy's port `25565` should be **Public** - it is the address players use. Leave the ports of the servers behind it unpublished, or set them to **Vibe Network**.

## Common problems

**The proxy cannot see a server.** Either there is no connection on the **Connections** card, or the address in `velocity.toml` is wrong. The address is the application's name in lower case, spaces as hyphens.

**A player joins and is immediately kicked.** Usually `online-mode`, or a mismatched forwarding secret. Check step 4.
