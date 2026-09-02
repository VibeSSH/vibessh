---
id: applications
title: Applications
section: applications
route: /applications
order: 10
---

An application is one thing running on a Node: a Minecraft server, a Velocity proxy, a database, a bot. VibeSSH runs its whole life cycle - creating it, starting it, configuring it, its files, its backups - and does that remotely over SSH, with no agent on the Node unless you install one yourself.

## Blueprints

An application is created from a blueprint: a ready-made recipe of which image, which ports, which configuration files, which variables. A blueprint is a starting point, not a cage - once the application exists you can change any of it.

The one thing a blueprint holds onto is ports marked **Required**: they can be edited but not removed.

## Runtimes

| Type | What it means |
| --- | --- |
| Docker | The application lives in a container on the Node. The default, and the best supported. |
| Systemd | A system service on the Node. |
| Local process | A process on this computer, not on a Node. |

The rest of this guide is mostly about Docker, because Docker is what concepts like recreating a container and published ports come from.

![An application's Overview: the console with severity-coloured lines, and resource usage beside it](images/app-overview.png)

## Operations

- **Start** - no confirmation. Starting risks nothing and undoes itself with one click.
- **Stop** - confirmed, because it drops everyone who is connected.
- **Restart** - stop and start the same container.
- **Kill** - ends the process without waiting. A game server will not save its world. A last resort, not a daily tool.
- **Recreate container** - removes the container and builds it again from the current configuration. **It does not touch data** - an application's files live outside the container.

### When a recreate is needed

Docker bakes part of the configuration into a container when it is created rather than reading it at each start: published ports, resource limits, environment variables, the image. A plain restart would reuse the same, now-stale container.

So changing any of those on a **running** application recreates its container automatically. A stopped application is left stopped and picks up the new configuration when it next starts.

## The console

The console on the Overview tab shows the application's output and lets you send commands to it.

For a Docker application on a Node over SSH it is a **real stream** (`docker logs -f`) - lines appear as the process writes them. For other runtimes the console polls the log every two seconds. The marker in the header says which is active: *Live* or *Polled*.

Lines are coloured by severity - warnings amber, errors red.

If the stream ends (a Node recycling its connection, say) the console retries a few times with a growing delay before falling back to polling.

## The tabs

- **Overview** - status, console, CPU and memory charts, the basic facts.
- **Files** - the application's file browser and editor.
- **Logs** - a fuller read of the log than the console window.
- **Ports** - what is exposed, and to whom.
- **Databases** - the databases attached to this application.
- **Backups** - copies of the working directory.
- **Settings** - configuration, image, resource limits, environment variables, health check.

## Migrating to another Node

Moves the application and its data to another Node. It takes a while and needs both Nodes reachable. It is not a way to clone - the source stops owning the application.

## Common mistakes

- **I changed the configuration and nothing happened** - if the application was stopped, the change is waiting for a start. If it was running, the container was recreated.
- **The console shows nothing** - check the application is running. A stopped container produces no output, and previous lines are gone once the container has been recreated.
- **Kill instead of Stop** - a game server will not have time to save its world. Stop normally unless the process has stopped responding.
