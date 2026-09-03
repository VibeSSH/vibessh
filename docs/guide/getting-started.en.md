---
id: getting-started
title: Getting started
section: getting-started
route: /
order: 1
---

From an empty app to a running server in five steps.

## 1. Add a server

1. Open **Servers**.
2. Click **Add server**.
3. Fill in the fields and click **Test connection**.
4. Click **Save server**.

Details: the **Servers** topic.

## 2. Set the Node up

1. On the server card, click the setup icon.
2. Next to anything missing, click **Install automatically**.
3. Wait until every requirement reads **Installed**.

## 3. Create an application

1. Open **Applications**.
2. Click **Create application**.
3. Go through the five wizard steps and confirm.
4. On the application's page, click **Start**.

## 4. Open a port

1. Open the application -> the **Ports** tab.
2. Check the port has the right **Network access**.
3. Click **Sync firewall**.

## 5. Turn on backups

1. Open the application -> the **Backups** tab.
2. Tick **Back up automatically**.
3. Set the interval and how many copies to keep.
4. Save.

## How to check it works

- The Node reads **Online**.
- The application reads **Running**.
- The **Overview** tab shows a console with log lines.
- Players connect to the server's IP address and the port from the **Ports** tab.

## Common problems

- **I cannot create an application** - Docker is missing on the Node. Go back to step 2.
- **The port does not answer from the internet** - check **Network access** and click **Sync firewall**. Check your VPS provider's own firewall too.
- **I changed a configuration file and nothing happened** - restart the application with **Restart**.
