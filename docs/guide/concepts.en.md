---
id: concepts
title: Nodes and applications
section: getting-started
route: /
order: 0
---

The rest of this guide rests on two ideas. They are worth five minutes, because confusing them is where most misunderstandings start: "I removed the container and lost my world", "I changed the configuration and nothing happened", "why do I need Vibe Network when the servers are all mine".

## A Node

**A Node is a machine VibeSSH manages.** A VPS, a server in a rack, a box in a cupboard. One machine, one Node.

VibeSSH is not installed on it. It connects over SSH - exactly as you would from a terminal, with your account and your key. Everything you see in the app is the result of commands run on that machine on your behalf.

Three consequences follow, and they recur throughout this guide:

- **Your privileges on a Node are your SSH account's privileges.** If the account has no `sudo`, neither has VibeSSH.
- **What VibeSSH knows about a Node, it knows from asking.** "Offline" means "I could not connect", not "the machine is dead".
- **Changes are real.** Stopping a service on the Actions tab stops it, along with everything that depended on it.

The other mode - the **Vibe Agent** - installs a small service on the machine and talks to it over its own protocol. It is a convenience where SSH is awkward; a Node in that mode reports no CPU or memory and offers no terminal from the app.

### What a Node is not

It is not an account in a VibeSSH service, and not anything we host. It is your machine, at your provider, on your bill. VibeSSH manages it and keeps nothing on it that you did not ask for.

## An application

**An application is one thing running on a Node.** A Minecraft server, a Velocity proxy, a bot, a database. One game server is one application - even if five of them share a Node.

An application has three layers, and telling them apart is the single most useful distinction in VibeSSH:

| Layer | What it is | Does it survive |
| --- | --- | --- |
| The container | The process and its runtime | **No.** Rebuilt on every configuration change. |
| The working directory | The world, plugins, configuration, logs | **Yes.** On the Node's disk, outside the container. |
| The declaration | Ports, limits, variables, image | **Yes.** Held by VibeSSH and applied to the Node. |

> **The container is disposable; the data is not.** "Recreate container" sounds alarming and is not - it removes and rebuilds the first layer without touching the second. Your Minecraft world lives in the working directory and was never inside the container.

This explains behaviour that otherwise looks like a bug: Docker bakes part of the configuration into a container **when it is created**, rather than reading it at start. Published ports, resource limits, environment variables, the image. So changing any of them on a running application **recreates the container** - a plain restart would reuse the same, now-stale one.

### What an application is not

It is not a Docker container. The container is how the application happens to run - the Actions tab lists a Node's containers and will happily stop the one belonging to an application, but the application is managed in its own view, with its console, ports and backups.

## How they relate

An application lives on **exactly one Node**. Moving it elsewhere is a migration: the data travels with it, and the source stops owning it.

A Node holds **as many applications as you like**. They cannot see each other until you connect them - each gets its own Docker network, and where a dedicated account is enabled, its own system account (`vibessh-app-...`). That is not cosmetic: without it every application on a Node could read every other one's files.

**Vibe Network joins Nodes, not applications.** It is a private WireGuard tunnel between machines, so an application on one server can reach a database on another without exposing that database to the internet. The tunnel alone does not let applications reach each other - that is granted separately.

## Blueprints

**A blueprint is a recipe for an application**: which image, which ports, which configuration files. A starting point rather than a cage - once the application exists you can change any of it. The one thing a blueprint holds onto is ports marked **Required**: they can be edited but not removed.

## In short

- **Node** - a machine. Yours, at your provider, managed over SSH.
- **Application** - one thing running on that machine. The container is disposable; the working directory is not.
- **Blueprint** - the recipe an application starts from.
- **Vibe Network** - a private network **between Nodes**.
