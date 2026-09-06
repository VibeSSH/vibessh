---
id: app-nodejs-bot
title: Node.js bot
section: blueprints
route: /applications
order: 250
---

Runs your own Node.js program - a Discord bot, a script, a small API. VibeSSH keeps it running, shows its logs, and restarts it when you say so.

VibeSSH does not write the code for you. This application takes the files you upload and runs the entry file you name.

## Step by step

1. **Applications - New application**, choose the location and a name, e.g. `bot`.
2. Pick **Node.js bot** from the list of types.
3. **Entry file** - the path relative to the working directory, e.g. `index.js` or `src/bot.js`.
4. **Node.js version** - leave `22` unless your code needs another.
5. **Program arguments** - usually empty.
6. Create the application, but **do not start it yet**.
7. **Files** tab - upload the bot's code along with `package.json`.
8. **Settings - Environment** - add the bot token. Tick **Secret** next to it: the value then goes to the operating system's credential store rather than VibeSSH's database, and is never shown on screen afterwards.
9. Press **Start**.

## Dependencies from package.json

This application runs `node YOUR-FILE`. It does not run `npm install` for you.

The simplest route: install the dependencies on your own machine and upload `node_modules` along with the code. Alternatively use the **Actions** tab or the Node's terminal to run `npm install` in the application's working directory.

## Updating the code

1. Upload the new files on the **Files** tab.
2. Press **Restart**.

## Common problems

**`Cannot find module`.** `node_modules` is missing - see the section above.

**The bot starts and immediately stops.** Look at **Logs**. Usually the token is missing from the environment variables, or the entry file is at a different path than the one you gave.

**The token shows up in the logs.** Do not print it from your code. A variable marked **Secret** never appears in the VibeSSH interface, but nothing stops your own `console.log`.
