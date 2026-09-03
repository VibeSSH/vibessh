---
id: teams
title: Teams
section: getting-started
route: /teams
order: 150
---

Teams let you give other people access to VibeSSH. They need a cloud account; the rest of the app works locally.

## How to create a team

1. Open **Teams**.
2. Enter a team name.
3. Click **Create**.

## How to add somebody

1. Open the team -> the **Members** tab.
2. Scroll to the **Create an account** card.
3. **E-mail** - that person's address.
4. **Name** - optional.
5. **Role** - choose a role from the list.
6. Click **Create account**.
7. Copy the password shown and pass it to them.

The password is shown once. If you lose it, create the account again.

They sign in with it and must change it immediately. Until they do, their account can do nothing else.

## How to create a role

1. Open the team -> the **Roles** tab.
2. Click **Create role**.
3. Enter a **Name** and an optional **Description**.
4. Tick permissions in the groups.
5. Save.

## How to share a server with the team

1. Open the team -> the **Servers** tab.
2. Add the server by entering its details.

Operation permissions only apply to servers shared with the team.

## How to check it works

- The person appears in the **Members** list.
- Their role is shown next to them.
- Once signed in, they see only the buttons their role allows.

## Common problems

- **I do not see Teams in the menu** - you have to be signed in.
- **An account with this email already exists** - that person already has an account. Add them as a member instead.
- **A new member can still do everything** - the server is not shared with the team, or they are using the same computer as you and can see your local servers.

## More detail

Operation permissions (applications, ports, files, firewall, terminal) hide and disable actions inside VibeSSH. They will not stop somebody who has SSH access to the server outside the app. A real boundary is a separate account on the server with limited `sudo`.
