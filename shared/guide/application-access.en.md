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

## How to take access away

Turn the switch off beside their name. When you turn off the last one, the application goes back to being visible to the whole team - the list is empty again, and the note says so.

## How to check it works

- Once you have added at least one person, the note above the list reads **Visible only to the people listed**.
- The people you turned on show a switch that is on; everyone else is off.

## Common problems

- **The tab says it needs the updated backend** - your account backend does not have this feature yet. It starts working on its own once it does; nothing here is broken.
- **I cannot turn any switch on** - you do not have permission to manage sharing in this team. Someone who does can give it to you.
- **I am the only name in the list** - invite people to your team first. Access is given to team members.

## Good to know

This decides who *sees* an application inside VibeSSH. It is not a security boundary: anyone who can reach the server over SSH sees the application regardless. What it is good for is keeping people who should not be touching something from stumbling into it by accident, which is a different and useful thing.
