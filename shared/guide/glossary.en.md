---
id: glossary
title: Glossary
section: getting-started
route: /guide
order: 4
---

The words from the interface and this guide that do not explain themselves.
Two sentences each — enough to know what you are looking at.

For the basics — what a Node, an application, a port or the firewall is —
see **Basic terms**. This page covers the private-network and firewall words
that appear in the interface without explanation.

## Blueprint

A template that says **what kind of thing** an application is: which image to
run or which file to execute, which fields to ask for when creating it, and
which tabs make sense. Paper, MariaDB and Redis are three different
blueprints.

## Bind address

The network address a program **listens on** — that is, where it can be
reached from at all, before a firewall enters the picture.

`127.0.0.1` means "only from this same machine". `0.0.0.0` means "on every
address this machine has". That is why a port published on `0.0.0.0` is
reachable from the internet until a firewall narrows it — and why the
**Ports** tab puts a badge beside each port saying whether that narrowing is
actually in force.

## Internal and external port

**Internal** is the port a program listens on inside its own container.
**External** is the port it can be reached on from outside the Node.

They are usually the same, but need not be: MariaDB might listen on `3306`
inside and be published on `3307`. Firewall rules are written against the
external one, because that is the one anything can connect to.

## Peer

The other end of a WireGuard tunnel. In Vibe Network every Node is a peer of
every other — with three Nodes, each has two peers.

## Endpoint

The address and port a peer can be reached at **from outside**, on the public
internet — `203.0.113.10:51820`, say. WireGuard needs it to know where to
send the first packet.

Do not confuse it with a `10.77.0.x` address: the endpoint is the way *to*
the tunnel, and `10.77.0.x` is an address *inside* it.

## Handshake

The exchange that confirms two peers have actually agreed with each other.
**A recent one confirms the two peers have exchanged traffic lately** — which
an assigned address and a saved configuration cannot tell you on their own.
It says nothing about what runs over the tunnel, so whether a name resolves
or a port accepts you are separate checks.

WireGuard renews it periodically, so a "last handshake" from a minute ago is
the normal state rather than a problem.

## Reconcile

Bringing what is on the server into line with what VibeSSH has recorded. A
reconcile does **not** re-apply everything: it compares the two and changes
only the differences, so it can be run repeatedly without harm.

That is what **Sync network** and **Sync firewall** do. It is also why both
are safe to press a second time when you are not sure the first one took.

## Source CIDR

The notation that says **which addresses** are allowed to connect.
`10.77.0.0/16` means "only from Vibe Network"; no such restriction means
"from anywhere".

It is the difference between a port your own servers can see and a port the
whole internet can see.

## Port visibility

The setting you choose on a port, from which VibeSSH derives both the bind
address and the firewall rule:

- **Public** — the entire internet.
- **Vibe Network only** — your own Nodes on the private network, and nothing
  else.
- **Localhost only** — the same machine, and nothing else.
- **Custom** — an address you type yourself.

## Trust On First Use

The rule by which VibeSSH checks an SSH host key: on the first connection it
shows the fingerprint and asks you to accept it, and from then on it **makes
sure it does not change**.

A changed key can mean the server was rebuilt — or that somebody has put
themselves in the middle of the connection. So VibeSSH refuses and waits for
a deliberate decision from you.
