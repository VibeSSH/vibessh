---
id: app-postgres
title: PostgreSQL
section: blueprints
route: /applications
order: 215
---

PostgreSQL is a database server. Web applications, bots and panels that need more than a file reach for it, and outside the Minecraft world it is the usual choice.

This Application is **your own database server in a container**, with its data in the Application's working directory. That is a different thing from **Databases** in the sidebar, where VibeSSH creates databases on a server installed directly on the Node. If you are not sure which you want, read "Which one to pick" at the bottom.

## Before you start

Have an administrator password ready. PostgreSQL **will not start without one** - the container comes up, prints an error and stops.

The second thing is less obvious and expensive to get wrong, so it is worth understanding even though the template handles it for you. The PostgreSQL image creates the database wherever the **PGDATA** variable points. If you do not set it, the database is created **inside the container** - it works perfectly, and it disappears the first time the container is recreated, which changing the version alone does. So `PGDATA` has to point at the Application's directory.

The value is **a single dot**: `PGDATA=.` - not `./pgdata` or any sub-path. By the time the image creates that directory it is already running as the `postgres` user, and an Application's working directory does not belong to that account, so it cannot make a subdirectory and the container will not come up.

## Step by step

1. **Applications → New application**.
2. In the first step, under **Templates**, pick **PostgreSQL with a password and a data directory**. The template fills in `PGDATA` and the names of the other variables, so you do not have to remember them.
3. Choose the location (this computer or a Node) and give it a name, for example `database`.
4. **PostgreSQL version** - leave `17` unless the application that will use it needs an older one.
5. In the **Environment** step, fill in the values:
   - **POSTGRES_PASSWORD** - the administrator password. Required.
   - **POSTGRES_DB** - the name of a database to create straight away, for example `app`.
   - **PGDATA** - leave the `.` the template filled in.
6. Create the Application and click **Start**.

The first start takes longer than the ones after it - the server is creating its files. Check the **Logs** tab: the line `database system is ready to accept connections` means it is up.

If you create the Application without the template, the working directory has to be **empty**. PostgreSQL refuses to create a database in a directory that already has something in it.

## Connecting to it

The database listens on port **5432** inside the container.

**From another Application on the same Node:**

1. Open the other Application → the **Ports** tab → the **Connections** card.
2. Pick the PostgreSQL Application and click **Connect**.
3. In that other Application's configuration, give the database's address as the **PostgreSQL Application's name in lower case**, with spaces replaced by hyphens. `My Database` becomes `my-database`. The port is `5432` and the user is `postgres` by default.

Without steps 1-2 nothing connects. Every container has its own private network and cannot see the others until you grant the connection - deliberately, so one compromised Application cannot reach all the rest.

**From outside, say your own SQL client on a laptop** - add a port on the **Ports** tab: internal `5432`, external whichever is free. Set the access to **Vibe Network**, not public. A database published publicly is attacked within hours.

## Queries without leaving the app

This Application's **Console** tab runs `psql` inside the container. Type a query - `SELECT version();` or `\dt` - and you get the server's answer. You do not need the password: the console connects over the local socket, which the server trusts.

## Common problems

**The container starts and stops immediately.** Usually `POSTGRES_PASSWORD` is missing. If it is there and the log says `Permission denied` while creating a directory, `PGDATA` is pointing at a subdirectory. Change it to `.`.

**The data disappeared after a version change.** `PGDATA` was not set, so the database lived inside the container. A container recreated after a settings change starts from nothing. Set `PGDATA=.` and create the database again - the earlier data cannot be recovered, because it never reached the Node's disk.

**"database files are incompatible with server".** The database was created by an older PostgreSQL than the one now starting. PostgreSQL does not upgrade the format on its own. Go back to the previous version in **Settings**, take a dump with `pg_dump`, then load it into the new one.

**You forgot the password.** It cannot be read back - it is stored as a secret and VibeSSH does not know it either. Change it in the database from the **Console** with `ALTER USER postgres PASSWORD 'new';`, then update the environment variable so the two agree.

## Which one to pick: this Application or Databases

Take the **PostgreSQL Application** when you want a separate, isolated database server - for one project, with its own version and its own directory that can be backed up along with everything else.

Take **Databases** in the sidebar when you want one shared server on the Node and databases created on it for several applications in one click. VibeSSH generates the database name, user and password, and manages access itself.
