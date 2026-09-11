---
id: app-purpur
title: Purpur
section: blueprints
route: /applications
order: 110
---

Purpur is a Minecraft server built on Paper, with extra gameplay settings - mob behaviour, mechanics, small things Paper leaves alone.

Everything in the **Paper** topic applies here in the same way: plugins, files, ports, console. Purpur understands all of it, because it is Paper with additions.

## Step by step

1. **Applications - New application**, choose the location and a name.
2. Pick **Purpur** from the list of types.
3. **Purpur version** - pick from the list. The number matches the Minecraft version.
4. **EULA accepted** - tick it, or the server will not start.
5. **Java version** - leave `21` unless something asks for newer.
6. **JVM arguments** - this is where memory is set, e.g. `-Xmx4G`.
7. Create it and press **Start**.

Port `25565` is added for you.

## How it differs from Paper

Purpur adds a `purpur.yml` next to `paper-global.yml`. You will find it on the **Files** tab. Changes to it need a restart.

If you are not sure whether you need Purpur, take Paper. Purpur is worth it when you want to change gameplay mechanics without writing a plugin.

## Common problems

The same as Paper's: the EULA, the Java version, a published port. See the **Paper** topic.
