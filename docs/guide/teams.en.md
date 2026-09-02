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

## Team servers

A server shared with a team is visible to its members according to their roles. SSH credentials stay where they were - sharing a server does not distribute your private key or password to anybody.

## Removing

Removing a member takes away their access to the team's resources. Deleting a team cannot be undone and affects every member of it.

## Common mistakes

- **I don't see Teams in the menu** - you have to be signed in. Everything else in VibeSSH works without an account.
- **I invited someone and nothing happened** - an invitation is a code the other person has to paste on their side.
- **A member cannot see a server** - check the server is shared with the team and that their role allows it.
