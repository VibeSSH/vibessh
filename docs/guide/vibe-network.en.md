---
id: vibe-network
title: Vibe Network
section: network
route: /vibe-network
order: 40
---

Vibe Network is a private network joining your Nodes over a WireGuard tunnel. It is what lets an application on one server talk to a database on another without exposing that database's port to the internet.

## Where it is

The sidebar -> **Vibe Network**. Three tabs: **Nodes**, **Endpoints**, **Private DNS**.

![Two Nodes in the network, both with an active tunnel and a recent handshake](images/vibe-network-nodes.png)

## Joining a Node

**Add node** takes a server from the list and does the whole job on it: installs `wireguard-tools` if they are missing, generates a key pair, allocates an address on the private network, and reconciles the configuration with every other member.

**The private key never leaves the Node.** It is generated on its own disk and only the public key comes back to the app.

A separate interface (`wg-vibessh0`) and an unusual UDP port are used, so as not to collide with WireGuard that may already be on that server.

## What a Node card says

| Row | Meaning |
| --- | --- |
| DNS name | The name the Node answers to on the private network. |
| Connection | The state of the **tunnel**, not of SSH. Values below. |
| Latency | How long the Node takes to answer. |
| Applications | How many run on it. |
| Endpoints | How many ports it exposes. |
| Last handshake | When WireGuard last agreed keys with a peer. |

The **Online / Offline** badge at the top says whether the Node answered SSH. That is a separate question from the state of the tunnel, which is why it is a separate indicator.

### Connection states

- **Active** - the tunnel is up and the handshake is recent.
- **Not established** - the tunnel is up but has never handshaked.
- **Idle** - there was a handshake, but a while ago.
- **No tunnel** - the interface is not on the Node. It has not joined, or has not been reconciled since joining.
- **Unknown** - the state could not be read. The reason appears in the **Tunnel status** row below.
- **Node not answering** - it could not be reached, so nothing was checked.

> The configuration sets `PersistentKeepalive = 25`, so **a working tunnel handshakes within half a minute of coming up, whether or not anybody sends traffic**. "Not established" that persists is a real problem, not an absence of traffic.

An **Unknown peers** row appears when a Node reports a peer whose key belongs to no member of this network. That is what a Node re-keyed outside the app looks like.

## Synchronising

**Sync Vibe Network** brings everything to the declared state: it reconciles WireGuard peers on every Node, applies firewall rules, and pushes the private DNS entries. Every Node and each of those three steps runs independently - one unreachable Node does not block the others.

The result appears as a list under the button, with a reason next to each failure.

## Private DNS

Gives Nodes and applications names like `vps.vibe`, so configuration can point at a name rather than at an address that changes when something moves.

## Endpoints

A view of every application port, gathered per Node. It is the same data as an application's Ports tab seen from the other side - there is nothing separate to configure here.

## Common mistakes

- **Sync says it succeeded but the connection says "Not established"** - synchronising distributes configuration; it is not what performs a handshake. Check that WireGuard's UDP port is allowed by your VPS provider's firewall.
- **The applications still can't see each other** - a tunnel alone does not grant that. Connections between applications are granted separately, on an application's Ports tab.
- **I removed a Node from the network and its key is gone** - it is not. Leaving takes down the interface and its config, but the key pair stays, so rejoining does not change the Node's identity.
