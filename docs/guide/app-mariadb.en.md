---
id: app-mariadb
title: MariaDB
section: blueprints
route: /applications
order: 210
---

MariaDB is a database server. Minecraft plugins, shops, panels and almost every web application keep their data in one.

This application is **your own database server in a container**, with its data in the application's working directory. That is a different thing from **Databases** in the sidebar, where VibeSSH creates databases on a server installed directly on the Node. If you are not sure which you want, read "Which one to pick" at the end.

## Before you start

Have a password ready for the `root` user. MariaDB **will not start without one** - the container comes up, prints an error about initialization, and stops. That is the most common reason a fresh database "doesn't work".

## Step by step

1. **Applications - New application**.
2. In the first step, under **Templates**, pick **MariaDB with a root password**. The template fills in the variable names the image needs, so you do not have to know them by heart.
3. Choose where it runs (this computer or a Node) and give it a name, e.g. `database`.
4. **MariaDB version** - leave `11` unless you have a reason to pick an older one.
5. In the **Environment** step, fill in the values:
   - **MYSQL_ROOT_PASSWORD** - the administrator password. Required.
   - **MYSQL_DATABASE** - a database to create straight away, e.g. `app`.
   - **MYSQL_USER** and **MYSQL_PASSWORD** - an ordinary account for that database.
6. Create the application and press **Start**.

The first start takes longer than the rest - the server is creating its files. Check the **Logs** tab: the line `ready for connections` means it is up.

## How to connect to it

The database listens on port **3306** inside its container.

**From another application on the same Node** - a Minecraft server, or phpMyAdmin:

1. Open the other application, go to the **Ports** tab, find the **Connections** card.
2. Pick the MariaDB application and press **Connect**.
3. In that application's configuration, use the **MariaDB application's name in lower case** as the database address, with spaces turned into hyphens. `My Database` is `my-database`. The port is `3306`.

Without steps 1-2 nothing will connect. Every container has its own private network and cannot see the others until you grant the connection - deliberately, so one compromised application cannot reach all the rest.

**From outside, e.g. an SQL client on your laptop** - add a port on the **Ports** tab: internal `3306`, external any free one. Set access to **Vibe Network**, not public. A database exposed to the internet is attacked within hours.

## Common problems

**The container starts and immediately stops.** `MYSQL_ROOT_PASSWORD` is missing. Add it under **Settings - Environment** and start it again. Note that `MYSQL_DATABASE`, `MYSQL_USER` and `MYSQL_PASSWORD` only take effect on the **first** start, when the data files are created. After that, new accounts are made inside the database itself.

**The other application cannot see the database.** Check, in order: that the connection exists on the **Connections** card, that the address is the application's name in lower case, and that the port is `3306` rather than whatever you published externally.

**You forgot the root password.** It cannot be read back - it is stored as a secret and VibeSSH does not know it either. Set a new value and change the password inside the database with `ALTER USER`.

## Which one to pick: this application or Databases

Take the **MariaDB application** when you want a separate, isolated database server - one project, its own version, its own directory that gets backed up along with everything else.

Take **Databases** from the sidebar when you want one shared server on the Node and databases created on it for many applications in one click. VibeSSH generates the database name, the user and the password, and manages access itself.
