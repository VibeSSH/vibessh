---
id: concepts
title: Basic terms
section: getting-started
route: /
order: 0
---

A short glossary of the names used throughout this guide.

## Node

A server or VPS added to VibeSSH. One machine is one Node.

## Application

One thing running on a Node: a Minecraft server, a proxy, a bot, a database. A Node can run many applications.

## Image

The recipe an application is created from. In the wizard the field is called **Image** (Paper, Velocity, and so on).

## Working directory

The folder on the Node holding the application's files: the world, plugins, configuration. You browse it on the **Files** tab.

## Port

The number a service is reachable on. Minecraft is usually `25565`.

## Vibe Network

A private network between your Nodes. It lets applications on different servers reach each other without exposing ports to the internet.

## Firewall

Controls which of a Node's ports are reachable from the internet.

## More detail

An application has two parts: the container (the process) and the working directory (the data). The container is rebuilt when configuration changes; the working directory is untouched. That is why **Recreate container** does not delete your world or plugins.
