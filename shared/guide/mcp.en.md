---
id: mcp
title: Claude and other assistants
section: getting-started
route: /settings
order: 145
---

VibeSSH can answer an AI assistant running on your own computer - Claude in a code editor
or in a terminal, for instance. The assistant asks what servers and applications you have
and reads their logs, so when you ask "why won't this server start?" it is looking at the
real log rather than guessing.

It is **off** by default and nothing is listening.

## What an assistant can see

- your servers' names, addresses, ports and login,
- your applications: type, working directory, status and which server each runs on,
- the most recent log lines from an application you name.

## What it can never see

- **passwords, private keys or database passwords** - those stay in the operating
  system's keyring and do not leave it, even if you ask,
- **an application's environment variables** - including the ones not marked secret,
  because an ordinary variable is where a licence key ends up too.

## How to turn it on

1. **Settings → Claude and other assistants**.
2. Turn on **Answer assistants on this computer**.
3. Copy the **Address** - it looks like `http://127.0.0.1:7422/mcp`.
4. Click **Show** beside the **Token** and copy it.
5. Put both into your assistant's configuration as an MCP server. In Claude Code:

```bash
claude mcp add --transport http vibessh http://127.0.0.1:7422/mcp --header "Authorization: Bearer PASTE_TOKEN"
```

6. Ask it something simple, such as "what servers do I have in VibeSSH?".

## How to allow restarts

A separate switch, **Allow changes**, which appears only once the endpoint is on. That is
what gives the assistant a tool for restarting an application.

The split is deliberate: "may Claude see my servers" and "may Claude restart them" are two
different questions, and most people answer them differently. While changes are off the
assistant **cannot even see** such a tool, so it cannot keep offering one.

## Running it from IntelliJ (Gradle)

Instead of a local dev server, you can push the freshly built plugin straight to a server
in VibeSSH and restart it - without leaving the editor.

Needs **Allow changes** on, because it writes a file to a server.

1. Get the application id - ask your assistant "what applications do I have in VibeSSH?".
2. Add **two imports at the very top** of `build.gradle.kts`, above the `plugins` block:

```kotlin
import java.net.HttpURLConnection
import java.net.URI
```

Not decoration. Inside `build.gradle.kts` the name `java` belongs to Gradle's own
extension (`JavaPluginExtension`), not to the Java package - so writing `java.net.URI`
out in full fails with `Unresolved reference 'net'`. The imports are what let you write
`URI` and `HttpURLConnection` instead.

3. Add the task, anywhere in the same file:

```kotlin
val vibesshDeploy by tasks.registering {
    dependsOn(tasks.shadowJar) // or tasks.jar
    doLast {
        val jar = tasks.shadowJar.get().archiveFile.get().asFile
        val app = providers.gradleProperty("vibesshApp").get()
        val token = providers.gradleProperty("vibesshToken").get()
        val url = URI(
            "http://127.0.0.1:7422/deploy?application=" + app +
                "&path=plugins/" + jar.name + "&restart=true"
        ).toURL()

        with(url.openConnection() as HttpURLConnection) {
            requestMethod = "POST"
            doOutput = true
            setRequestProperty("Authorization", "Bearer " + token)
            setRequestProperty("Content-Type", "application/octet-stream")
            outputStream.use { jar.inputStream().copyTo(it) }
            check(responseCode == 200) {
                "VibeSSH refused: " + responseCode + " " +
                    (errorStream?.readBytes()?.decodeToString() ?: "")
            }
            println("VibeSSH: " + inputStream.readBytes().decodeToString())
        }
    }
}
```

4. In `gradle.properties` - **not in the repository**, since it holds a token:

```properties
vibesshApp=PASTE-APPLICATION-ID
vibesshToken=PASTE-TOKEN
```

5. In IntelliJ, double-click `vibesshDeploy` in the Gradle panel. You can also add the
   task to a run configuration so one shortcut does it.

The log after the restart is in VibeSSH, on that application's **Logs** tab - or ask your
assistant for it.

### Things to watch

- **The token ends up in `gradle.properties`, which is a file.** Keep it out of the
  repository (add it to `.gitignore`) or put it in `~/.gradle/gradle.properties`, shared
  across projects.
- **`restart=true` restarts a real server.** On a production one, prefer `false` and
  restart deliberately, when nobody is playing.
- **The path is relative to the application's working directory.** Anything trying to
  climb out of it is refused.
- **VibeSSH has to be running.** Hidden in the tray is fine; closed with **Quit VibeSSH**
  is not.

## Security, in plain terms

**This endpoint never leaves your computer.** That is not a setting, because there is no
good reason for a list of your servers to be reachable from a network.

**The token is the password to it.** Without one, anything running on this computer could
reach it. Treat it like any other password - do not paste it into a chat, do not leave it
on a screenshot. That is why it is hidden until you ask for it.

**If the token gets out**, click **Issue a new token**. The old one stops working
immediately rather than at the next launch, and any assistant holding it simply loses
access.

## How to check it works

- Ask your assistant "what servers do I have in VibeSSH?" - it should list the ones on
  your Servers page.
- Ask for the last few log lines of an application and compare them with the Logs tab.

## Common problems

- **The assistant says it cannot connect** - check VibeSSH is running. The endpoint exists
  only while the app does: closing the window to the tray keeps it, **Quit VibeSSH** does
  not.
- **"couldn't open the local endpoint ... port in use"** - something else has 7422. Change
  the port in Settings and update the address in your assistant.
- **The assistant offers a restart and is refused** - **Allow changes** is off.
- **The assistant does not see the new token** - after issuing one you have to paste it
  into the assistant again; the old one is void immediately.
- **`Unresolved reference 'net'` on `java.net.URI`** - inside `build.gradle.kts` the name
  `java` belongs to Gradle's extension, not to the Java package. Add `import java.net.URI`
  and `import java.net.HttpURLConnection` at the very top and use the short names.
