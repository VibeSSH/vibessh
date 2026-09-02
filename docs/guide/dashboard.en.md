---
id: dashboard
title: Dashboard
section: getting-started
route: /
order: 2
---

The dashboard answers one question: is anything asking for my attention. If nothing is, you come here for a second and leave.

## The header

The overall state: **All good** or **Needs attention**, and how many Nodes are online. It is derived from the alerts below, so when it reads as a warning the reason is on the same page.

## Nodes

A tile per Node, carrying CPU, memory, uptime and its Vibe Network sync state.

- **In sync** - the configuration on the Node matches what the app declares.
- **Out of sync** - something has drifted. **Sync** brings that one Node to the intended state.
- **Collecting...** - the Node answers but there is no first metrics sample yet. A CPU percentage is the difference between two readings, so the first has nothing to compare against.
- **Offline** - the Node did not answer.

## Alerts

The things worth knowing without looking for them: a server offline, a Node unreachable on Vibe Network, an application reporting failure. An empty list is real information, not missing data.

## Tasks

Outstanding work that one click can do - usually a Node waiting to be synchronised. The section appears only when there is something in it.

## The tabs underneath

Once a Node tile is selected:

- **Applications** - what runs on it.
- **Terminal** - a shell to that Node without going to the Terminal module. SSH Nodes only.
- **Activity** - recent events. Nodes in Agent mode have no CPU/memory metrics, only a sync state.

**All Nodes** clears the selection and returns to the overview.

## How often this refreshes

Node metrics every few seconds, the dashboard as a whole less often. Refreshing stops while the window is hidden and resumes the moment it comes back - each reading is an SSH connection to a Node, not a free request.

## Common mistakes

- **The dashboard says offline but the server is up** - check that VibeSSH can reach it over SSH. The dashboard reports what it managed to do, not whether the machine is alive.
- **The metrics are frozen** - if the window was hidden, the last reading is from before it was hidden.
