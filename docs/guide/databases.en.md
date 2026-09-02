---
id: databases
title: Databases
section: applications
route: /database-hosts
order: 45
---

VibeSSH creates MySQL/MariaDB databases for applications and looks after their credentials. It is not a database manager - there is no table browsing and no query editor; phpMyAdmin is for that, and you can reach it from here.

## Two levels

A **database host** is the engine: a MySQL or MariaDB server on which databases are created. You register one once, from the sidebar under **Databases**.

An **application database** is a specific database created for one application on a chosen host. You create it in the application, on its Databases tab.

With no host registered the tab in an application has nothing to offer, and says so plainly.

## Registering a host

You give the address, the port and an administrative account for the engine. That account needs the right to create databases and users - VibeSSH uses it for nothing else.

If a Node has no engine yet, an **Install MariaDB** button is available. That is an operator's deliberate decision rather than something that happens by itself while creating an application: installing a database server changes the Node permanently.

## Creating a database for an application

You pick a host, optionally give a purpose (`luckperms`, say), and that is all. **The database name, user and password are generated** - you do not invent them and you do not have to record them anywhere.

A separate account per database is the point rather than decoration: an application gets access to its own database and to nothing else.

## Credentials

The eye button shows the host, database name, user and password - to paste into a plugin's or an application's configuration.

**The password is not kept in the application's configuration.** It is shown on request. If you lose it you do not recover it - you generate a new one with the reset button, which changes the password in the engine immediately.

> After resetting a password you have to update the application's configuration and restart it. Nothing does that for you - the old password stops working the same instant.

## phpMyAdmin

If a host has phpMyAdmin configured, the button opens it for this database. It is a hand-off to an external tool; VibeSSH does not sit between you and your queries.

## Deleting

Deleting a database drops it in the engine, with its data. It cannot be undone and there is no backup of it here - application backups cover the working directory, not database contents.

## Common mistakes

- **"No database host registered"** - add a host from the sidebar first, then create the database in the application.
- **The application cannot connect** - check that it can see the host. A database on another Node needs a Vibe Network connection or an exposed port; both Nodes being yours does not by itself give them connectivity.
- **I reset the password and the server broke** - the application's configuration still has the old one. Update it and restart.
