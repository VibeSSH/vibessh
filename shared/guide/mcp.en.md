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
