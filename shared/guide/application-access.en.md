---
id: application-access
title: Who can see an application
section: applications
route: /applications
order: 60
---

Sharing an application with a team shows it to everyone on that team. The **Users** tab narrows that to specific people: someone you have not added does not see the application in their list at all.

## Before you start

This uses your VibeSSH account and a team. If you are not signed in, or you are not in a team yet, the tab says so and points you to where to set that up. Access is given to people who are members of your team.

## How to give someone access

1. Open the application and go to the **Users** tab.
2. If you are in more than one team, pick the team at the top.
3. Find the person in the list and turn on the switch beside their name.

The first person you add changes what "shared" means for this application. Until then it is visible to the whole team; from the first switch on, it is visible only to the people you have turned on, and to you, since you shared it. The note above the list always says which of the two is true right now.

## How to give someone permissions on this application

Turning the switch on lets someone view the application and read its logs. For them to do anything with it, tick permissions under their name:

- **Start, stop and restart** - start, stop, restart and force-end it.
- **Console (type commands)** - type commands into its console. On a Minecraft server that is any command, `op` included, so give it only to people you trust.
- **Read files** - browse, open and download its files.
- **Edit and upload files** - change its files. Ticking this ticks reading too.

These permissions apply to this one application only. Someone who may restart one server cannot touch another one on the same Node.

After changing permissions, open **Teams**, your team, the **Servers** section, and run the access sync for the Node the application runs on. That is when the permissions reach the server.

## What the person you shared with sees

1. They open **Teams**, your team and the **Servers** section, and click **Add to my servers** beside the Node. VibeSSH then connects as their own account, not yours.
2. Your application appears in their **Applications** list, marked **Shared**.
3. When they open it, they see only what you allowed: viewing, the logs, and the buttons and tabs of the permissions you ticked. The top of the page says what they can do.

They cannot delete it or change its settings, ports or backups - those always stay with you.

## How to take access away

Turn the switch off beside their name. When you turn off the last one, the application goes back to being visible to the whole team - the list is empty again, and the note says so.

## How to check it works

- Once you have added at least one person, the note above the list reads **Visible only to the people listed**.
- The people you turned on show a switch that is on; everyone else is off.

## Common problems

- **The tab says it needs the updated backend** - your account backend does not have this feature yet. It starts working on its own once it does; nothing here is broken.
- **I cannot turn any switch on** - you do not have permission to manage sharing in this team. Someone who does can give it to you.
- **I am the only name in the list** - invite people to your team first. Access is given to team members.
- **My teammate does not see the application in their list** - check that they added the Node to their servers from the team page, not by hand with another account. The application appears only where they connect as their own account.
- **My teammate is told they do not have permission** - the permission is not ticked, or the access sync has not run since it was.

## Good to know

The permissions on an application are held by the server itself: your teammate's account on the Node can run exactly the commands you gave it and nothing more, outside VibeSSH too. Being on the list is a different matter - anyone who can reach the server over SSH can still see that the application is there.
