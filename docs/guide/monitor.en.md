---
id: monitor
title: Monitor
section: nodes
route: /monitor
order: 65
---

Monitor shows what a Node is doing right now: resource usage, network throughput and the process list. It is a live reading, not a history system - data accumulates while the page is open.

## Where it is

The sidebar -> **Monitor**, once a server is chosen.

## Resources

Four readings, refreshed every few seconds: **CPU**, **RAM**, **Network in** and **Network out**.

CPU and network throughput are **differences between two readings**, not values the system reports directly. That is why the first reading after arriving says "collecting..." - there is nothing yet to compare it against.

## History

A chart of the last several minutes, built from the moment the page opened. Nothing persists it: leave and come back and it starts again. A chart implying a past nobody recorded would be worse than an empty one.

## Processes

The running processes sorted by memory use, with PID, user, CPU, memory and command.

Sorting by memory is deliberate: a process eating memory is the most common reason a Node suddenly slows down, or the kernel kills a game server.

## Refreshing

The page polls the Node every few seconds and **stops while the window is hidden**, resuming as soon as it returns. Each reading is an SSH connection - a monitor left open overnight would otherwise cost thousands of pointless ones.

## Common mistakes

- **I closed the page and the history was gone** - that is intended; nothing persists it. Long-term observation needs a dedicated tool on the Node.
- **CPU reads 0%** - that is most likely the first sample. Wait for the second.
- **Memory looks entirely used** - Linux spends free memory on disk cache and gives it back when it is needed. What matters is memory held by processes, not "free".
