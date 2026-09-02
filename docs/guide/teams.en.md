---
id: teams
title: Teams
section: getting-started
route: /teams
order: 85
---

Teams let you share access to servers with other people. It is the only part of VibeSSH that needs a cloud account - the rest of the app works locally and requires no sign-in at all.

## Where it is

The sidebar -> **Teams**. The entry appears once you are signed in.

## A team

You create a team by giving it a name. Whoever created it is the **owner**.

## Inviting

You invite someone and they receive an **invitation code**. They paste it into "Got an invitation code?" on their side and accept or decline.

An invitation is a code rather than an automatic addition: until the other person uses it, nothing has happened.

## Roles

Members have roles that decide what they can do with the servers shared in the team. Roles are set on the member, on the team's page.

## Operation permissions - and what they are not

Beyond permissions over the team itself (members, roles, invitations), roles also carry permissions over operations: creating applications, ports, files, backups, the firewall, the terminal, installing software, Vibe Network.

> **These are guard rails, not a security boundary.** They hide and disable actions inside VibeSSH, so a new member does not click something by accident. They cannot stop somebody who does not want to be stopped.

The reason is architectural and worth knowing: these operations **do not pass through the backend**. The desktop app performs them over its own SSH connection, with that person's credentials, from their machine. The backend holds team server metadata only and never holds credentials. Anybody with SSH access to a Node can do the same thing with plain `ssh`, without VibeSSH.

The permissions apply only to servers **shared with a team**, matched by address and port. Your own servers, in no team, are subject to nothing.

If the app is signed out or cannot reach the backend, nothing is restricted. That is deliberate: a network failure must not lock you out of your own machines.

### When you need a boundary that holds

Give that person a **Node account** with limited `sudo` and their own SSH key. Linux enforces it rather than the interface, so it applies outside VibeSSH too. The two complement each other well: permissions organise everyday work, the account draws the line.

## Team servers

A server shared with a team is visible to its members according to their roles. SSH credentials stay where they were - sharing a server does not distribute your private key or password to anybody.

## Removing

Removing a member takes away their access to the team's resources. Deleting a team cannot be undone and affects every member of it.

## Common mistakes

- **I don't see Teams in the menu** - you have to be signed in. Everything else in VibeSSH works without an account.
- **I invited someone and nothing happened** - an invitation is a code the other person has to paste on their side.
- **A member cannot see a server** - check the server is shared with the team and that their role allows it.
