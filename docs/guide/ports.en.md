---
id: ports
title: Ports
section: applications
route: /applications
order: 30
---

A port is a declaration of which network sockets an application uses and who may connect to them. VibeSSH does not infer this from the container - you declare it, and the Node is brought to that state on every change.

## Where it is

Application -> the **Ports** tab. The card at the top lists the declared ports; below it, behind a rule, sits the Node firewall sync.

## The form's fields

| Field | What it does | Example |
| --- | --- | --- |
| Name | The label in the list. It reaches no server configuration anywhere. | `Minecraft` |
| Protocol | TCP or UDP. Minecraft Java is TCP; Bedrock and most Source games are UDP. | `TCP` |
| Access | Who may connect. Covered below. | `Public` |
| Internal port | The port the process listens on **inside the container**. | `25565` |
| External port | The port on the Node. Leave it empty to use the same number. | `25566` |

The external port earns its place when two applications on one Node listen on the same number internally. Two Paper servers can both be `25565` inside and `25565` and `25566` outside - neither container needs to know.

## Access - four levels

This is the field that decides exposure, so the badge on a port is coloured: public reads as a warning, Vibe Network as green, the rest as neutral.

- **Public** - bound to `0.0.0.0`, reachable from the whole internet. What you want for a game port players connect to.
- **Vibe Network** - reachable only from the Node's address on the private network. An admin panel or an API you connect to yourself belongs here, not in public.
- **Localhost** - bound to `127.0.0.1`. Visible only to processes on the same Node.
- **Custom address** - you give the bind address yourself, for the cases the three above do not cover.

> Choosing an access level does not by itself close the port to the world - the Node's firewall does that. After changing access, use **Sync firewall** rather than waiting.

## What happens when you save

A Docker container's published ports are baked in when it is created (`docker create -p`), not read at start. A plain restart would reuse the same, now-stale container, so **changing a port on a running application recreates its container**. A stopped application is left stopped.

Recreating a container does not touch data - an application's files live outside it.

## Node firewall sync

The button at the foot of the card. It derives the rules for every application on that Node and applies them: the SSH port, the WireGuard port, and every application port scoped to its access level. Rules that are no longer wanted are removed.

The result can report three uncomfortable things, and they are worth telling apart:

- **No firewall backend** - the Node has neither `ufw` nor `nftables`. Nothing is enforcing anything.
- **Rules not enforced** - a backend exists but is inactive. This is the worst case, because the sync "succeeded" while a port marked "Vibe Network only" is publicly reachable.
- **Node unreachable** - no rules were applied at all.

## Required ports

A port marked **Required** comes from the application's blueprint. It can be edited but not removed - the same rule the backend itself enforces, not just the interface.

## Common mistakes

- **The port works locally but not from outside** - check that access is `Public` and sync the firewall. If it still fails, check the firewall at your VPS provider; VibeSSH has no access to that one.
- **"Port is taken" when saving** - another application or process on the Node holds that number. Change the external port.
- **You changed the port and players still hit the old one** - the change recreates the container only while the application is running. If it was stopped, the container is created with the new configuration when it next starts.
