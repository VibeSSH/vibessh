import { describe, expect, it } from "vitest";
import { portProtection } from "./portProtection";
import type { ApplicationPort } from "@/types/application";
import type { NodeFirewallOverview } from "@/services/serverService";

function port(overrides: Partial<ApplicationPort>): ApplicationPort {
  return { id: "p1", name: "game", protocol: "tcp", bindAddress: "0.0.0.0", internalPort: 25565, visibility: "vibeNetwork", ...overrides } as ApplicationPort;
}

function overview(overrides: Partial<NodeFirewallOverview>): NodeFirewallOverview {
  return {
    backend: "ufw",
    active: true,
    rules: [],
    // A Node with no Docker by default: then the ufw rule is the whole
    // story, which is what the cases below that predate container rules
    // assume.
    container: { applicable: false, restrictedPorts: [], error: null },
    ...overrides,
  } as NodeFirewallOverview;
}

const RULE = { port: 25565, protocol: "tcp" as const, origin: { kind: "application" as const, applicationId: "a", applicationName: "paper", portName: "game" } };

/**
 * Whether a port is really restricted, shown per port and without pressing
 * anything.
 *
 * A published Docker port is bound widely and only the Node's firewall
 * narrows it. The app knew this and said so - but only as a summary, after a
 * Sync Firewall press, once for the whole tab. A "Vibe Network only" port
 * that nothing is enforcing is open to the internet, which is not a fact to
 * find out by pressing a button that sounds like it changes something.
 */
describe("portProtection", () => {
  it("calls an enforced rule protection", () => {
    expect(portProtection(port({}), overview({ rules: [RULE] }))).toBe("protected");
  });

  /** The case that made "Vibe Network only" ports publicly reachable: the
   *  rules are stored, the firewall is installed, and it is switched off. */
  it("calls a stored rule nobody enforces unprotected", () => {
    expect(portProtection(port({}), overview({ active: false, rules: [RULE] }))).toBe("unprotected");
  });

  it("calls a node with no firewall at all unprotected", () => {
    expect(portProtection(port({}), overview({ backend: null, active: false, rules: [RULE] }))).toBe("unprotected");
  });

  it("calls an enforcing firewall with no rule for this port unprotected", () => {
    expect(portProtection(port({ internalPort: 8080 }), overview({ rules: [RULE] }))).toBe("unprotected");
  });

  /** A public port is meant to be reachable. Calling that "unprotected"
   *  would be describing the intention as a fault, and a red badge on every
   *  correctly-published port trains people to ignore red badges. */
  it("says nothing about protection for a port that is meant to be public", () => {
    expect(portProtection(port({ visibility: "public" }), overview({ rules: [] }))).toBe("public");
  });

  /** Loopback is refused by the kernel at the socket, whatever the firewall
   *  is doing - see resolve_bind_address. */
  it("counts a loopback port as protected without consulting the firewall", () => {
    expect(portProtection(port({ visibility: "localhost" }), null)).toBe("protected");
  });

  /** A local application has no Node firewall to ask about, and a Node that
   *  has not answered yet has not said "protected". Guessing either way
   *  here is the dangerous guess. */
  it("admits it does not know rather than guessing", () => {
    expect(portProtection(port({}), null)).toBe("unknown");
    expect(portProtection(port({}), undefined)).toBe("unknown");
  });

  /** A published container port is reached from outside on its external
   *  port; matching the rule against the internal one is how a rule that
   *  exists reads as missing. */
  it("matches the rule against the published port, not the container's own", () => {
    const published = port({ internalPort: 25565, externalPort: 25570 });

    expect(portProtection(published, overview({ rules: [RULE] }))).toBe("unprotected");
    expect(portProtection(published, overview({ rules: [{ ...RULE, port: 25570 }] }))).toBe("protected");
  });

  it("does not confuse udp with tcp on the same number", () => {
    expect(portProtection(port({ protocol: "udp" }), overview({ rules: [RULE] }))).toBe("unprotected");
  });
});

describe("a published Docker port", () => {
  const published = () => port({ externalPort: 25565 });

  /// The fault this exists to stop. Docker writes its own DNAT and ACCEPT
  /// ahead of ufw's chains, so a published port stays reachable however
  /// correct `ufw status` looks - and the badge used to be decided from the
  /// ufw rule alone, telling somebody their database was closed while it was
  /// answering the internet.
  it("is not protected by a ufw rule alone", () => {
    const state = portProtection(
      published(),
      overview({ rules: [RULE], container: { applicable: true, restrictedPorts: [], error: null } }),
    );
    expect(state).toBe("unprotected");
  });

  it("is protected once the container chain really restricts it", () => {
    const state = portProtection(
      published(),
      overview({ rules: [RULE], container: { applicable: true, restrictedPorts: [25565], error: null } }),
    );
    expect(state).toBe("protected");
  });

  /// After DNAT the chain sees the container's own port, so a rule on that
  /// number is the one that counts for a remapped port.
  it("counts a restriction on the container's own port when the two differ", () => {
    const remapped = port({ internalPort: 80, externalPort: 8080 });
    const rule = { ...RULE, port: 8080 };
    const state = portProtection(
      remapped,
      overview({ rules: [rule], container: { applicable: true, restrictedPorts: [80], error: null } }),
    );
    expect(state).toBe("protected");
  });

  /// Unknown is not protected. One sends somebody to look; the other sends
  /// them away satisfied.
  it("is unknown, not protected, when the chain could not be read", () => {
    const state = portProtection(
      published(),
      overview({ rules: [RULE], container: { applicable: true, restrictedPorts: [], error: "couldn't read the DOCKER-USER chain" } }),
    );
    expect(state).toBe("unknown");
  });

  /// A Node with no Docker has nothing bypassing ufw, so the ufw answer
  /// stands on its own and this must not start reporting false alarms.
  it("is protected on a Node with no Docker at all", () => {
    const state = portProtection(published(), overview({ rules: [RULE] }));
    expect(state).toBe("protected");
  });
});
