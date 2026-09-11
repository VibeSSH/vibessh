---
id: app-redis
title: Redis
section: blueprints
route: /applications
order: 230
---

Redis is a very fast cache and key-value store. Bots use it for sessions, panels for job queues, server networks for keeping instances in sync.

It keeps its data mostly in RAM. It is not a replacement for MariaDB - it suits things you can afford to lose on a restart, and things that must be read in a fraction of a millisecond.

## Step by step

1. **Applications - New application**, choose the location and a name, e.g. `redis`.
2. Pick **Redis** from the list of types.
3. **Redis version** - leave `7`, or pick `8`.
4. **Password** - set one. An empty field means a server with no authentication at all.
5. Create the application and press **Start**.

## Password and access

Redis with no password is safe **only** while its port is not published anywhere. A bot in another container cannot reach it without a granted connection either, so an empty field is not a hole by itself - it becomes one the moment you publish the port.

If you do publish it, set a password and choose **Vibe Network** access. A Redis exposed publicly with no password is taken over automatically, within minutes.

## Connecting to it from another application

1. Open the application that will use Redis, go to the **Ports** tab, find the **Connections** card.
2. Connect it to the Redis application.
3. In that application's configuration use the address: **the Redis application's name in lower case**, port `6379`.

## Command console

The application's **Overview** tab has a **Command console**. You type a Redis command and get the answer back - no SSH, no hunting for `redis-cli`.

1. Open the application, go to the **Overview** tab.
2. Type a command, e.g. `KEYS *`, `GET key`, `INFO memory`.
3. Press Enter, or **Run**.

The up and down arrows walk back through earlier commands.

The password is supplied for you: VibeSSH reads it **inside the container**, from the running server's own arguments, so it never appears in any command run on your machine or on the Node.

Each command runs on its own. For ordinary use that makes no difference, but `SELECT 1` will not carry over to the next command.

The application has to be running - `docker exec` has nothing to attach to in a stopped container.

## Common problems

**`NOAUTH Authentication required`.** You set a password and the application is not sending it. Add it to that application's configuration.

**Connection refused.** Either no connection on the **Connections** card, or the wrong address - it is the application's name, not `localhost`.

**Data disappears after a restart.** That is how Redis works in this setup. For anything that has to survive, use a MariaDB database.
