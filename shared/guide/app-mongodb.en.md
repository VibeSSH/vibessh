---
id: app-mongodb
title: MongoDB
section: blueprints
route: /applications
order: 215
---

MongoDB is a document database. Instead of tables and columns it stores JSON-like documents, so the fields do not have to be decided up front.

Bots, panels and Node.js applications use it - anywhere the shape of the data changes over a project's life. If your plugin or application asks for "MySQL" or "MariaDB", this is not it; see the **MariaDB** topic.

## Before you start

Have an administrator username and password ready. In MongoDB those two variables do more than they look like: the image creates the administrator account from them **and only setting them turns authentication on**. A database started without them accepts anyone who can reach it, with no password at all.

## Step by step

1. **Applications - New application**.
2. Under **Templates**, pick **MongoDB with an administrator account**. The template fills in both variables above.
3. Choose where it runs (this computer or a Node) and give it a name, e.g. `mongo`.
4. **MongoDB version** - leave `8` unless your application needs an older one.
5. In the **Environment** step, fill in the values:
   - **MONGO_INITDB_ROOT_USERNAME** - the administrator's name, e.g. `root`.
   - **MONGO_INITDB_ROOT_PASSWORD** - their password. Leave **Secret** ticked.
6. Create the application and press **Start**.

The data lives in the application's working directory, so a backup of the application is a backup of the database.

## How to connect to it

The database listens on port **27017** inside its container.

**From another application on the same Node:**

1. Open that application, go to the **Ports** tab, find the **Connections** card.
2. Pick the MongoDB application and press **Connect**.
3. In its configuration use an address of this shape:

```
mongodb://USER:PASSWORD@APPLICATION-NAME:27017/DATABASE?authSource=admin
```

`APPLICATION-NAME` is the MongoDB application's name **in lower case**, with spaces turned into hyphens. `My Database` is `my-database`.

Without steps 1-2 nothing will connect: containers cannot see each other until you grant it.

**From outside, e.g. your own client** - add a port on the **Ports** tab: internal `27017`, external any free one, access **Vibe Network**. A public MongoDB with no password is found, scraped and wiped by bots within hours - this has happened at scale.

## Command console

The application's **Overview** tab has a **Command console**. You type a `mongosh` expression and get the answer back.

1. Open the application, go to the **Overview** tab.
2. Type a command, e.g. `db.getMongo().getDBNames()` or `db.getSiblingDB("shop").users.countDocuments()`.
3. Press Enter, or **Run**.

The up and down arrows walk back through earlier commands.

The administrator's name and password are substituted **inside the container**, from the environment variables it already holds. They never reach a command VibeSSH runs, so they cannot show up in a process list.

Each command is its own run of the client, so **`use some-database` does not carry over**. Use `db.getSiblingDB("some-database")` within the same command instead.

The application has to be running - a stopped container has nothing to attach to.

## Common problems

**`Authentication failed`.** Usually `?authSource=admin` missing from the connection string. The account from `MONGO_INITDB_ROOT_USERNAME` is created in the `admin` database, not in the one you are connecting to.

**The database lets you in with no password.** The variables were set after the first start. Both only take effect on the **first** start, when the data files are created. Create the account with `db.createUser` from a client instead.

**An application cannot reach the database.** Check in order: the connection on the **Connections** card, the address as the application's name in lower case, and port `27017` - the internal one, not whatever you published externally.

**The container starts and stops.** Look at **Logs**. If the working directory already holds data from a newer MongoDB, an older version will refuse to open it.
