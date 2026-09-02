---
id: terminal
title: Terminal
section: nodes
route: /terminal
order: 60
---

The terminal is an ordinary shell on a Node - the same thing you would get by running `ssh`, in an app window and with tabs.

## Where it is

The sidebar -> **Terminal**, once a server is chosen. The same thing is a click away on the Dashboard, on a selected Node's Terminal tab.

## Tabs

**New terminal** opens another tab. Each tab is its own SSH session with its own history and its own working directory - closing one does not touch the others.

Tabs do not survive closing the app. These are interactive sessions, not `screen` or `tmux`; if you need a process that outlives a disconnect, run it under `tmux` on the Node or make it an application.

## Search

The **Search the terminal** box searches the current tab's scrollback, with previous and next match.

## When a session ends

When a session ends - you typed `exit`, the server closed it, the connection dropped - the tab stays with a message and the reason, when one is known. **The tab does not reconnect by itself.** Close it and open a new one; reconnecting silently would put you back in a different state from the one you left.

## SSH only

The terminal works for Nodes in SSH mode. A Node in Agent mode does not offer a shell from this app.

## A note about Vibe AI

The assistant **has no terminal access** and runs no commands. It can describe what to do; you type it. That is a design boundary, not a missing feature.

## Common mistakes

- **I started a server in the terminal and it vanished when I closed the tab** - a process started in an interactive session dies with it. That is what applications are for.
- **The terminal is not in the menu** - no server is selected, or the Node is in Agent mode.
- **Copy and paste** - it behaves like a terminal, not like a text editor; selecting copies, right-click pastes.
