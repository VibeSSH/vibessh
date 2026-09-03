---
id: databases
title: Databases
section: applications
route: /database-hosts
order: 60
---

VibeSSH creates MySQL/MariaDB databases for applications and looks after their credentials.

![A database created for an application, with its generated name and user](images/databases-tab.png)

## How to add a database host

A host is the database server the databases are created on. You add one once.

1. Open **Databases** in the sidebar.
2. Click **Add host**.
3. Enter the address, port and an administrative account for the database server.
4. Save.

If a Node has no database engine yet, click **Install MariaDB** and wait for it to finish.

## How to create a database for an application

1. Open the application -> the **Databases** tab.
2. Choose the database host in the selector.
3. **Purpose** - optional, e.g. `luckperms`.
4. Click **New database**.

The database name, user and password are generated for you.

## How to copy the connection details

1. Click the eye icon on the database.
2. You will see **Host**, **Database**, **User** and **Password**.
3. Copy them into the plugin's or application's configuration.

## How to delete a database

1. Click the bin icon on the database.
2. Confirm.

Deleting drops the database with its data. It cannot be undone.

## How to check it works

- The database is in the list with its name and user.
- The plugin or application connects without an error.

## Common problems

- **No database host registered** - add a host from the **Databases** entry in the sidebar first.
- **The application cannot connect** - check that the database's port is set to **Vibe Network only** and that both Nodes are on Vibe Network.
- **I reset the password and it stopped working** - update the password in the application's configuration and restart it.

## More detail

Each database gets its own user, so an application can reach only its own. The password is not stored in the application's configuration - you reveal it on request. Application backups do not include database contents.
