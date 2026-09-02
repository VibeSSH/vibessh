---
id: vibe-ai
title: Vibe AI
section: ai
route: /vibe-ai
order: 80
---

Vibe AI answers questions about VibeSSH and helps diagnose a Node or an application that is misbehaving. It reads the same guide you are reading now, so its answers describe this version of the app rather than general Docker knowledge from the internet.

## What the assistant does not do

This matters as much as what it does:

- **It has no terminal access** and runs no commands. It can tell you what to type; you type it.
- **It changes nothing** - not configuration, not files, not the state of an application.
- **It does not run in the background.** There is no autonomous agent watching your Nodes.

## Two modes

**Ask** - answers questions about VibeSSH. **Nothing is read from your Nodes.** This is the mode for "what does recreating a container do" or "how does Vibe Network access differ from public".

**Diagnose** - additionally sends a snapshot of a chosen Node or application: status, recent logs, configuration. This is the mode for when something is broken and it is not obvious why.

## Exactly what gets sent

The **What will be sent** panel shows the precise content that goes with your message. Not a summary and not a description - the same text the model sees.

Before it is sent the snapshot is sanitised: **passwords, tokens, private keys, secret environment values and passwords inside connection strings are removed.** That includes values passed on a container's command line and authorization headers in logs.

Look at the panel when in doubt. It exists so that you do not have to take a description on trust.

## Configuration

**Settings -> AI**. The shared model (Qwen) works without a key of your own and has a daily per-user question limit - the usage counter is in the same place.

To use a different model you supply your own details: base URL, model name and API key. **The key goes into the operating system's credential store**, never into a configuration file, and it is not returned to the interface after saving. **Test connection** checks the settings work before you ask your first question.

## Ask Vibe AI from an error

Where an application reports a failure, an **Ask Vibe AI** button appears. It opens the assistant with the context already selected and the question already written, so you do not retype the message.

## How to read the answers

The assistant is meant to name **one likely cause** and a concrete next step, not to enumerate everything that could theoretically have gone wrong. If an answer spreads into a list of possibilities, there was usually too little context - try Diagnose with the right application selected.

An answer can be incomplete if you end it with **Stop**; it is clearly marked when that happens.

## Common mistakes

- **"The assistant is not configured yet"** - either nothing is set in Settings, or the shared model's daily limit is used up.
- **The answer does not know my application** - Ask mode reads nothing from your Nodes. Switch to Diagnose and choose the application.
- **I asked it to fix something** - the assistant performs no operations. It will describe what to do.
