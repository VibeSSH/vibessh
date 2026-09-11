---
id: app-generic-java
title: Java application
section: blueprints
route: /applications
order: 140
---

Runs any `.jar` file. For anything written in Java that is not Paper, Purpur, Velocity or Waterfall - a bot, a tool, a Minecraft server in a flavour VibeSSH does not download itself.

The difference from **Paper**: here you upload the `.jar` and manage its version yourself. VibeSSH downloads nothing.

## Step by step

1. **Applications - New application**, choose the location and a name.
2. Pick **Java application** from the list of types.
3. **Java version** - `21` suits most things. If the program complains about a `class file version`, raise it.
4. **Jar file** - the path relative to the working directory, e.g. `server.jar`.
5. **JVM arguments** - memory and flags, e.g. `-Xmx2G`. You can paste a whole `java -Xmx2G -jar server.jar nogui` line - VibeSSH splits it across the right fields.
6. **Program arguments** - what comes after the jar name, e.g. `nogui`.
7. Create the application, upload the `.jar` on the **Files** tab, press **Start**.

## Ports

No port is added automatically, because VibeSSH does not know what your program listens on. Add one yourself on the **Ports** tab and press **Sync firewall**.

## Common problems

**`Unable to access jarfile`.** The **Jar file** path is wrong, or the file was never uploaded. Check the **Files** tab - the name has to match exactly, extension included.

**`UnsupportedClassVersionError`.** The program needs newer Java. Raise the **Java version**.

**It runs, but nobody can connect.** There is no published port - see above.
