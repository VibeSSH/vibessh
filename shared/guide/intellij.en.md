---
id: intellij
title: The IntelliJ plugin
section: getting-started
route: /settings
order: 146
---

The plugin sends a built file straight from the IDE to an application in VibeSSH and
restarts it. Instead of a local dev server you work against the same server the plugin
will actually live on.

It also works without the plugin, through the Gradle task described on the **Claude and
other assistants** page. There is one difference and it matters: **the plugin keeps the
token in the IDE's credential store**, while the Gradle task keeps it in a file that is
usually inside a repository.

## Before you start

1. VibeSSH **0.1.0-beta.17** or newer, **running** (hidden in the tray counts).
2. **Settings → Claude and other assistants** turned on, together with **Allow changes**,
   because the plugin writes a file to a server.

## Installing

The plugin is not on the Marketplace yet, so it installs from a file.

1. Build the package:

   ```bash
   cd apps/intellij
   ./gradlew buildPlugin
   ```

   That produces `build/distributions/vibessh-intellij-0.1.0.zip`.

2. In IntelliJ: **Settings → Plugins → ⚙ → Install Plugin from Disk...** and pick it.
3. Restart the IDE.

If it refuses to load, build it against your own IDE:

```bash
./gradlew buildPlugin -PvibesshIdePath="C:/Program Files/JetBrains/IntelliJ IDEA 2026.1"
```

The compiler then checks that every API the plugin uses exists in that release, instead of
letting it install and throw on first use.

## Setting it up

**Settings → Tools → VibeSSH**:

| field | what to put there |
|---|---|
| Port | `7422`, unless you changed it in VibeSSH |
| Token | from **Settings → Claude and other assistants → Token → Show** |
| Target directory | `plugins` for Minecraft plugins |
| Restart after deploy | leave on for a test server |

Click **Test connection**. A good answer reads "Connected to vibessh 0.1.0-beta.17" and
means three things at once: VibeSSH is running, the endpoint is on, and the token is
right.

The token goes into the IDE's credential store, backed by the operating system's keychain.
**It never lands in `.idea/` or any project file**, so it cannot reach a repository.

## Deploying

**Build → Deploy to VibeSSH.**

1. Pick the file - the chooser opens in `build/libs` when that directory exists.
2. Pick the application from the list VibeSSH returns.
3. Progress appears in the status bar; the result arrives as a notification.

The application is chosen on every deploy rather than in settings, because getting it
wrong means a plugin landing on a server it was never meant for, and that server being
restarted into. The last choice is preselected.

## Things to watch

- **The restart is of a real server.** On a production one, turn **Restart after deploy**
  off and restart deliberately, when nobody is playing.
- **The path is relative to the application's working directory.** Anything climbing out
  of it is refused on the VibeSSH side.
- **After issuing a new token** you have to paste it into the plugin's settings again -
  the old one stops working immediately.

## How to check it works

- **Test connection** returns the VibeSSH version.
- After a deploy, **Files** in VibeSSH shows the file in the target directory with
  today's date.
- The application's **Logs** show it starting with the plugin loaded.

## Common problems

**"Paste the token first"** - the token field in the plugin's settings is empty.

**"Could not connect to VibeSSH"** - the app is not running, or the endpoint is off.
Hidden in the tray is running; closed with **Quit VibeSSH** is not.

**"VibeSSH refused the token"** - it does not match, usually after issuing a new one. Copy
it again.

**"VibeSSH has Allow changes switched off"** - the plugin writes a file to a server, so it
needs that permission. Turn it on in VibeSSH's settings.

**The plugin does not appear after installing** - restart the IDE; the plugins directory
is read at startup. If that does not help, build it with `-PvibesshIdePath` pointing at
your installation.
