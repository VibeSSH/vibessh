---
id: port-forwarding
title: SSH tunnels
section: nodes
route: /port-forwarding
order: 75
---

An SSH tunnel carries traffic over your connection to a Node. It is for reaching something that is not - and should not be - exposed to the world: a database console, an admin port, a service listening only on the Node's localhost.

This is a different thing from application ports and the firewall. There you declare what should be reachable permanently; here you open yourself a way through for a while.

## Where it is

The sidebar -> **Port forwarding**, once a server is chosen.

> Tunnels are **not saved**. They live only while VibeSSH is open, and must be opened again after a restart. That is deliberate - a permanent tunnel is in practice an open port somebody forgot about.

## Three kinds

**Local** - a port on your computer leads to an address as seen from the Node. The common case: `localhost:3306` on your machine reaches `127.0.0.1:3306` on the server, and a database client connects as though the database were local.

**Remote** - the reverse: a port on the server leads to an address as seen from your computer. Used when the server is the one that has to reach something of yours.

**Dynamic (SOCKS5)** - a proxy on your computer whose traffic leaves from the Node. You do not name one destination; the application using the proxy does.

## The fields

| Field | Meaning | Example |
| --- | --- | --- |
| Bind address | Where the tunnel accepts connections. `127.0.0.1` keeps it to you. | `127.0.0.1` |
| Bind port | The port it listens on. `0` means any free port. | `3306` |
| Target host | Where it leads, as seen from the other side. | `127.0.0.1` |
| Target port | The port on the other side. | `3306` |

For a local tunnel the target is resolved **on the Node**, not on your machine. That is why `127.0.0.1` means "that server" there rather than your computer - and why this works for services bound to the Node's localhost.

## Common mistakes

- **I entered a target as seen from my computer** - the target is resolved on the Node's side. Enter what the server sees.
- **The tunnel disappeared** - closing VibeSSH closes all of them. Nothing recreates them.
- **The bind port is taken** - something on your computer already holds it. Enter `0` and the system picks a free one.
- **I want permanent access** - this is not the tool for that. Permanent access is an application port with the right access level, ideally "Vibe Network".
