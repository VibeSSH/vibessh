---
id: app-paper
title: Paper
section: blueprints
route: /applications
order: 100
---

Paper is a Minecraft server. It is a development of Spigot - faster, with more settings, and it runs Bukkit/Spigot/Paper plugins.

VibeSSH downloads the server file itself and manages its version. You do not need to fetch anything by hand or upload it over FTP.

## Step by step

1. **Applications - New application**.
2. Choose the location: **this computer** or a Node. A server for friends usually belongs on a Node, so it keeps running when your computer is off.
3. Give it a name, e.g. `survival`. That name is also the address other applications on the same Node reach it by.
4. Pick **Paper** from the list of types.
5. **Minecraft version** - pick from the list, e.g. `1.21.11`. It has to match the version your players connect with.
6. **EULA accepted** - tick it. Without it the server starts once and stops, asking you to accept Mojang's licence.
7. **Java version** - leave `21`. Newer Minecraft needs newer Java; if the server complains about a `class file version`, raise this.
8. **JVM arguments** - leave empty, or paste a ready-made set from [flags.sh](https://flags.sh). This is where memory is set, e.g. `-Xmx4G`.
9. Create the application and press **Start**.

Port **25565** is added and published automatically, because nobody can connect without it. You can change it on the **Ports** tab.

## The first start

The **Console** tab shows the server coming up. `Done (12.345s)! For help, type "help"` means it is running and players can join.

The console is interactive - you can type server commands into it, e.g. `op YourName` or `stop`. Colours from chat and logs are shown as the server sends them.

## Plugins and configuration

- **Files - upload** - drop the plugin's `.jar` into the `plugins` directory, then press **Restart**.
- **Files - Quick files** - `server.properties`, `bukkit.yml`, `spigot.yml`, `paper-global.yml` and `paper-world-defaults.yml` are one click away, with no hunting through directories.
- A database for plugins (LuckPerms, shops) - see **MariaDB** or the **Databases** tab.

## Changing the version

Open **Settings - Configuration - Edit** and choose a different **Minecraft version**. On save, VibeSSH downloads the matching server file and recreates the container.

Take a backup on the **Backups** tab first. A world saved by a newer version usually will not open in an older one.

## Common problems

**The server stops right after starting, with something about the EULA in the log.** You did not accept the licence. Settings - Configuration - Edit - tick **EULA accepted**.

**`Unsupported class file major version` or `UnsupportedClassVersionError`.** A plugin, or the server itself, needs newer Java. Raise the **Java version** in the configuration.

**Players cannot join.** Check the **Ports** tab: that port `25565` is published and set to **Public**, then press **Sync firewall**. Give players the Node's address, not your computer's.

**The server eats the whole Node's memory.** Set a limit under **Settings - Resource limits** and match `-Xmx` in the JVM arguments to it.
