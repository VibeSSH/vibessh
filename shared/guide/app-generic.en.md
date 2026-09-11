---
id: app-generic
title: Generic application
section: blueprints
route: /applications
order: 160
---

Runs any command on a Node or on this computer - no Docker, straight as a process.

Choose this when you already have a program or a script and only want something to keep it running, show its logs, and restart it in one click.

## Step by step

1. **Applications - New application**, choose the location and a name.
2. Pick **Generic application** from the list of types.
3. **Command** - the full path to the executable, e.g. `/usr/bin/python3`.
4. **Arguments** - one per line, in order, e.g. `main.py`.
5. **Stop command** - optional. If the program needs an orderly shutdown (like `stop` on a Minecraft server), put it here. Without it VibeSSH sends an ordinary termination signal.
6. Create the application and press **Start**.

## The working directory

The command runs in the application's working directory. Relative paths in the arguments are resolved against it.

## How this differs from a container

The process runs directly on the machine: it sees its filesystem and its installed packages. It does not have a container's isolation, so there is no private network and no **Connections** card here either - it reaches other services by ordinary address and port.

## Common problems

**`No such file or directory`.** The **Command** field needs a full path. Find it in a terminal with `which name`.

**The program will not stop.** Fill in **Stop command**, or use **Force stop**.
