---
id: port-forwarding
title: SSH tunnels
section: nodes
route: /port-forwarding
order: 90
---

A tunnel lets you reach a service on a server that is not exposed to the internet - a database console, for instance.

## How to create a tunnel

1. Open **Port forwarding** and choose a server.
2. Click **New tunnel**.
3. **Tunnel type** - choose **Local**.
4. **Bind address** - enter `127.0.0.1`.
5. **Bind port** - a number on your computer, e.g. `3306`. Enter `0` to let the system pick a free one.
6. **Target host** - the address as seen from the server, usually `127.0.0.1`.
7. **Target port** - the service's port on the server, e.g. `3306`.
8. Click **Start**.

## Example

Reaching MariaDB on a server:

- Tunnel type: **Local**
- Bind address: `127.0.0.1`
- Bind port: `3306`
- Target host: `127.0.0.1`
- Target port: `3306`

Once it is running, your database client connects to `127.0.0.1:3306`.

## How to stop a tunnel

1. Find the tunnel in the list.
2. Click the stop icon.

## How to check it works

- The tunnel appears in the list of active ones.
- A program on your computer connects to the local port you gave.

## Common problems

- **The bind port is taken** - something on your computer already uses it. Enter `0`.
- **The tunnel disappeared after restarting the app** - tunnels are not saved. Create it again.
- **It will not reach the target** - **Target host** is resolved on the server's side. Enter the address the server sees, usually `127.0.0.1`.

## More detail

Tunnels only run while VibeSSH is open. For permanent access, use an application port with the right **Network access**. The **Remote** type works the other way: a port on the server leads to your computer. **Dynamic (SOCKS5)** creates a proxy whose traffic leaves from the server.
