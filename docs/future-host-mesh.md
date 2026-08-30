# Future: private host mesh (Etap L — not implemented)

This is a notes file, not a build target. Per the project plan, Etap L is
architecture-reservation only: don't implement a mesh network now, don't
write a custom WireGuard, just make sure nothing built earlier would need to
be torn up to add this layer later.

## Problem this would solve

Multiplayer game infrastructure (the VibeSSH Pro / Minecraft module's
territory) commonly needs servers to talk to each other directly — e.g. a
Paper backend registering with a Velocity proxy, or two game servers
exchanging player-transfer data — without routing that traffic through one
central hub server that becomes both a bottleneck and a single point of
failure. This came up in an informal discussion about exactly that
Pterodactyl-panel-style pain point: node-to-node networking is consistently
the hardest part of running a fleet of game servers, harder than anything
about the game software itself.

## Shape of a future solution

- Node-to-node connectivity between managed hosts, not hub-and-spoke through
  the desktop app or a relay server, for the common case
- A WireGuard-based overlay (conceptually like Tailscale) rather than a
  bespoke protocol — per the project rules, use an existing, audited
  implementation instead of writing our own crypto/tunneling
- Private/virtual addresses and MagicDNS-like names per host, so a Paper
  server can reference "velocity-proxy-1" instead of a real IP
- Ports for inter-node traffic should default to reachable only within the
  private mesh/local network, not exposed publicly — the panel would offer
  "open this port" scoped to "local network only" vs. "public internet" as
  distinct choices, not a single on/off toggle
- NAT traversal with relay fallback when direct peer connections aren't
  possible
- ACLs controlling which hosts can reach which

## Why this is deferred, not dismissed

Etap L exists specifically so this gets designed for, not bolted on:
`ConnectionMode`, the `ServerConnection` abstraction, and the agent's
`capabilities` module (Etap I) are all built so that a future `MeshTransport`
or a capability flag like `"mesh_member": true` could slot in without
touching SSH Mode or Agent Mode's existing code paths. When this gets built,
it most likely piggybacks on an existing library (e.g. a Rust WireGuard
userspace implementation, or embedding Tailscale's own open-source
`tailscale`/`tsnet` approach) rather than reimplementing tunneling.
