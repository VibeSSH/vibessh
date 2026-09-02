---
id: getting-started
title: Getting started
section: getting-started
route: /
order: 1
---

Five steps take you from an empty app to a running server. Each has its own topic here; this one gives the order, and what each step means for the next.

## 1. Add a server

**Servers -> Add server**. Address, user, password or key. Use **Test connection** before saving - it opens a real connection and saves nothing, so corrections cost you nothing.

The account should have `sudo`. Without it some things work, but installing Docker, firewall rules and the dedicated accounts applications use do not.

## 2. Create an application

**Applications -> New application**, choosing a blueprint and a Node. A blueprint is a starting point: image, ports, configuration files. All of it can be changed afterwards.

A newly created application is not running yet - **Start** does that.

## 3. Open a port

**Application -> Ports**. A blueprint usually declares a port at creation; check it has the right access level.

The access level is a declaration. The Node's firewall is what enforces it, so use **Sync firewall**.

> VPS providers often have their own firewall in front of the machine. VibeSSH cannot see it - if a port does not answer despite correct settings, check the provider's panel.

## 4. Configure it

**Application -> Files**. The editor highlights syntax, and checks YAML as well - a syntax error blocks saving, because a file like that would stop the server from starting.

After saving configuration, restart the application: most servers read their files only at start.

## 5. Turn on backups

**Application -> Backups**. Set the schedule before you need it.

Two things to know straight away: the schedule runs **only while VibeSSH is open**, and a backup on the same Node as the application does not protect you from losing that Node. The external destination (S3) is configured in Settings.

## Where to go next

- **Several servers that should see each other** -> the Vibe Network topic.
- **A database for an application** -> the Databases topic.
- **Something is broken and it is not obvious why** -> the Vibe AI topic, Diagnose mode.
- **Access to something that should not be public** -> the SSH tunnels topic.

## An order that saves trouble

1. Test the connection before saving a server.
2. Sync the firewall after every change to a port's access.
3. Restart the application after every change to a configuration file.
4. Back up before any change you cannot undo.
