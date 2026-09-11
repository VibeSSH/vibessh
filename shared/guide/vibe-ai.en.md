---
id: vibe-ai
title: Vibe AI
section: ai
route: /vibe-ai
order: 130
---

Vibe AI helps analyse errors and configuration.

## How to set it up

1. Open **Settings**.
2. Find the **Vibe AI** card.
3. Tick **Enable the Vibe AI assistant**.
4. **Provider** - choose from the list.
5. **Base URL** - the provider's API address.
6. **Model** - the model name.
7. **API key** - paste your key.
8. Run the connection test.
9. Save.

The shared model works without a key of your own and has a daily question limit.

## How to ask a question

1. Open **Vibe AI** in the sidebar.
2. Choose a mode:
   - **Ask** - answers about VibeSSH. Nothing is read from your servers.
   - **Diagnose** - additionally sends data about a chosen server or application.
3. In **Diagnose**, choose the server or application.
4. Type your question and press Enter.

## How to ask about an application's error

1. Open the application reporting the error.
2. Click **Ask Vibe AI**.
3. The assistant opens with the question written and the context selected.

## What will be sent

The **What will be sent** panel shows the exact content that goes with your message. Passwords, tokens, private keys and secret environment values are removed first.

## How to check it works

- The connection test succeeds.
- Sending a question produces an answer.
- In **Diagnose**, the **What will be sent** panel shows the chosen application's data.

## Common problems

- **The assistant is not configured** - nothing is set in **Settings**, or the shared model's daily limit is used up.
- **The answer does not know my application** - **Ask** mode reads nothing from your servers. Switch to **Diagnose**.
- **I asked it to fix something and nothing happened** - the assistant performs no operations. It describes what to do.

## More detail

The API key goes into the operating system's credential store and is not returned to the interface after saving. The assistant has no terminal access and does not run in the background.
