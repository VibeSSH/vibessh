---
id: applications
title: Applications
section: applications
route: /applications
order: 20
---

An application is one thing running on a Node: a Minecraft server, a proxy, a bot, a database.

![An application's Overview: the console with severity-coloured lines, and resource usage beside it](images/app-overview.png)

## How to create an application

1. Open **Applications**.
2. Click **Create application**.
3. **Step 1 - Location and basics**: under **Where should it run?** pick a Node or this computer. Enter a **Name** and a **Working directory**, e.g. `/srv/paper`. Above those fields is a **Templates** list - picking one fills in the rest of the wizard for you.
4. **Step 2 - Application type**: choose what this is (Paper, MariaDB, phpMyAdmin...) and a **Runtime**.
5. **Step 3 - Configuration**: the fields depend on the type - server version, Java version, entry file. phpMyAdmin also has a **Database** field here.
6. **Step 4 - Environment variables**: a template fills in the variable names the image needs. You can also skip this and add them later.
7. **Step 5 - Review**: check the details and confirm.
8. On the application's page, click **Start**.

The wizard does not set memory or ports. You do that afterwards, on the **Settings** and **Ports** tabs.

Not sure what a particular field wants? On the application's page, next to the name of its type, there is a question mark - it opens the step-by-step page for that exact type: what to set, how to connect to it, and what to do when it does not work.

## Templates

A template is a remembered set of wizard answers: the application type, its settings and its environment variables. It does not remember the location, because that is the one thing that usually differs each time.

VibeSSH ships a few - **MariaDB with a root password**, **phpMyAdmin for a MariaDB application**, **phpMyAdmin for any server**. They fill in the variable names without which the image will not start, or will not connect to anything. Built-in templates cannot be deleted or overwritten.

You save your own on the wizard's last step, with **Save as template**. Passwords and keys are **not saved** - only the variable's name survives, and the wizard asks for the value each time.

## Connections between applications

Applications on one Node **cannot see each other** until you allow it. That is deliberate: compromising one then does not hand over the rest.

1. Open the application, go to the **Ports** tab, find the **Connections** card.
2. Pick the other application and press **Connect**.

The connection works both ways. From then on one application reaches the other by its **name in lower case**, with spaces turned into hyphens - `My Database` is `my-database`. The port is the one the program listens on inside its container, not the one published externally.

This is the most common reason for "everything is configured correctly and it still will not connect": the entry here is missing.

## Changing an application's type

**Settings - Application type** switches an existing application to a different type, in either direction.

- **To a managed type** (Paper, Purpur, Velocity, Waterfall): VibeSSH downloads its own server file into the application's directory and manages the version from then on. Your worlds, plugins and configuration are untouched, but the server will start from the downloaded file. Stop it first.
- **To a plain Docker container**: nothing in the directory is downloaded, replaced or removed. Only the way it starts changes.

The warning above the button says which of those two you are about to do.

## Adopting servers that already exist

If a machine already has servers on it - left by a Pterodactyl panel, installed by hand, restored from a backup - you do not have to recreate them.

1. Open **Applications** and press **Adopt servers**.
2. Choose the location and the directory they are in (on a Pterodactyl host, usually `/home/container`).
3. Press **Scan**. VibeSSH lists what it found, along with each server's `.jar` and the port from its `server.properties`.
4. Untick the ones you do not want, choose a Java version, and press **Adopt**.

What you get is ordinary Docker containers pointed at the existing directories. Nothing is downloaded or replaced - deliberately, so adopting cannot overwrite a running server. If you later want VibeSSH to manage the version, use **Changing an application's type** above.

## Controlling an application

The buttons at the top of the application's page:

| Button | What it does |
| --- | --- |
| **Start** | Starts the application. |
| **Stop** | Shuts it down cleanly. |
| **Restart** | Stops and starts it. |
| **Kill** | Ends it immediately. A game server will not save its world. |
| **Recreate container** | Rebuilds the container. Does not delete the application's files. |
| **Migrate to another Node** | Moves the application with its data. |

## The console

The console is on the **Overview** tab.

1. Type a command in the field at the bottom.
2. Press Enter or click **Send**.

Warnings are amber, errors red.

## Setting memory and CPU

1. Open the **Settings** tab.
2. Find the **Resource limits** card and click **Edit**.
3. **Memory** - in MB, e.g. `2048`. Empty means no limit.
4. **CPU** - a number of cores, e.g. `1.5`.
5. Save.

## Environment variables

1. Open the **Settings** tab.
2. Click **Add variable**.
3. Enter a **Key** and a **Value**.
4. For passwords and API keys, tick **Secret**.
5. Save.

## How to check it works

- The application reads **Running**.
- New lines appear in the console.
- The **Resource usage** card shows CPU and memory.

## Common problems

- **I changed the configuration and nothing happened** - if the application was stopped, the change applies when it next starts.
- **The console is empty** - the application is not running, or the container was just recreated.
- **The application will not start** - open the **Logs** tab and read the last lines.

## More detail

Docker bakes ports, limits, variables and the image into a container when it is created. That is why changing any of them on a running application recreates the container automatically. The application's data lives in the working directory outside the container and is not touched.
