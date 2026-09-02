---
id: actions
title: Actions
section: nodes
route: /actions
order: 68
---

Actions shows what runs on a Node **outside** VibeSSH: systemd services and Docker containers you did not create here as applications.

Applications are managed in the Applications module. This screen is for the rest of the machine - system services, other people's containers, things set up by hand before VibeSSH existed.

## Where it is

The sidebar -> **Actions**, once a server is chosen.

## systemd services

A list of units with two independent states that are easy to confuse:

- **Active / Inactive** - whether the service is running **right now**.
- **Enabled / Disabled** - whether it comes back **after a reboot**.

These are separate things. A service can be running and not enabled (it disappears when the server reboots), or enabled and not running (it failed and is waiting for the next start).

The available operations are start, stop, restart, and enable or disable at boot. The name filter narrows the list - a typical server has over a hundred.

## Docker containers

The containers on the Node, with their state and a log view. An empty list means the Node has no containers, or no Docker.

## A word of caution

This screen acts on the Node's real system. Stopping a service something else depends on stops that too - and there is no confirmation describing the consequences, because VibeSSH does not know what a given unit does in your setup.

Be especially careful with networking and SSH: stopping `ssh` cuts VibeSSH off from the Node, and the only way back is your VPS provider's console.

## Common mistakes

- **I stopped an application's container here** - you can, but managing an application belongs in its own view, alongside its console, ports and backups.
- **The service runs but is gone after a reboot** - it was active but not enabled. Two different things.
- **I see no containers** - check that Docker is installed and that the account can reach it.
