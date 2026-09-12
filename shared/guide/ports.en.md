---
id: ports
title: Ports
section: applications
route: /applications
order: 30
---

A port makes a service reachable - a Minecraft server to its players, for instance.

![The Ports tab: three ports with their access badges, and the firewall sync at the foot of the card](images/ports-tab.en.png)

## How to add a port

1. Open the application -> the **Ports** tab.
2. Click **Add port**.
3. **Name** - a description for you, e.g. `Minecraft`.
4. **Protocol** - choose `TCP` or `UDP`.
5. **Network access** - pick one of the options below.
6. **Internal port** - the number the application listens on, e.g. `25565`.
7. **External port** - leave empty to use the same number.
8. Save.
9. Click **Sync firewall**.

## Network access

| Option | Who can connect |
| --- | --- |
| **Public** | Anybody on the internet. |
| **Vibe Network only** | Only your other Nodes. |
| **Localhost only** | Only processes on the same Node. |
| **Custom address** | You give the address yourself. |

## Examples

- Minecraft Java: `25565`, TCP, **Public**.
- Minecraft Bedrock: `19132`, UDP, **Public**.
- RCON: `25575`, TCP, **Vibe Network only**.
- MariaDB: `3306`, TCP, **Vibe Network only**.

Do not set a database to **Public** without a specific reason.

## How to check it works

- The port is in the list with the right access badge.
- Players connect to the server's IP address and that port number.
- Changing it on a running application recreates the container automatically.

## Common problems

- **The port does not answer from the internet** - set **Public**, click **Sync firewall**, then check your VPS provider's firewall. VibeSSH cannot see that one.
- **The port is taken** - another application uses that number. Enter a different **External port**.
- **I cannot delete a port** - a port marked **Required** comes from the image. It can be edited but not removed.

## More detail

The access level is a declaration; the Node's firewall enforces it, which is why a change needs **Sync firewall**. Docker bakes ports into a container when it is created, so changing one on a running application recreates the container. The application's files are untouched.
