---
id: vibe-network
title: Vibe Network
section: network
route: /vibe-network
order: 70
---

Vibe Network lets applications on different servers talk to each other privately, without exposing ports to the internet.

![Two Nodes in the network, both with an active tunnel and a recent handshake](images/vibe-network-nodes.png)

## How to add a Node to the network

1. Open **Vibe Network**.
2. Click **Add node**.
3. Choose a server from the list.
4. Wait until the **Connection** row reads **Active**.
5. Repeat for the other servers.

The Node needs WireGuard. If it is missing, install it from the Node setup screen with **Install automatically**.

## How to connect two applications

1. Open the application -> the **Ports** tab.
2. Scroll to the **Connections** card.
3. In **Connect to**, choose the other application.
4. Click **Connect**.

The connection works both ways and applies immediately.

## How to check it works

- Both Node cards read **Connection: Active**.
- The **Last handshake** row shows a time measured in seconds or minutes.
- The **Connections** card lists the other application with the name it is reachable as.

## How to check the whole network

1. Click **Sync Vibe Network**.
2. A list of Nodes with the result appears under the button.

## What the "Connection" row means

| Value | What it means |
| --- | --- |
| **Active** | The tunnel is working. |
| **Not established** | The tunnel is up but has never connected. |
| **Idle** | It connected, but a while ago. |
| **No tunnel** | The Node has no network interface for this. |
| **Unknown** | The state could not be read. The reason is in the row below. |
| **Node not answering** | The server did not respond. |

## Common problems

- **Sync says it succeeded but the connection says "Not established"** - check that your VPS provider is not blocking UDP. Vibe Network uses UDP port `54221`.
- **The applications still cannot see each other** - the tunnel joins servers, not applications. Add a connection on the **Connections** card.
- **No tunnel** - the Node has no WireGuard, or has not been synchronised yet.

## More detail

Vibe Network uses WireGuard. The private key is generated on the server and never leaves it. The **Private DNS** tab gives servers names like `vps.vibe`, so configuration can point at a name instead of an IP address.
