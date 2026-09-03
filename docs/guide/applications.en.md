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
3. **Step 1 - Location and basics**: under **Where should it run?** pick a Node. Enter a **Name** and a **Working directory**, e.g. `/srv/paper`.
4. **Step 2 - Image and runtime**: choose an **Image** (Paper, say) and a **Runtime**.
5. **Step 3 - Configuration**: choose the version. For Java servers, also choose a Java installation.
6. **Step 4 - Environment variables**: you can skip this and add them later.
7. **Step 5 - Review**: check the details and confirm.
8. On the application's page, click **Start**.

The wizard does not set memory or ports. You do that afterwards, on the **Settings** and **Ports** tabs.

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
