import type { ApplicationPort } from "@/types/application";
import type { NodeFirewallOverview } from "@/services/serverService";

/**
 * Whether a port is actually restricted right now, port by port.
 *
 * **Why per port, and why without pressing anything.** A published Docker
 * port is bound widely and only the Node's firewall narrows it - which the
 * documentation says honestly, and which a reviewer read as "the app never
 * tells you". It did tell you, but only as a summary after you pressed Sync
 * Firewall, and only once for the whole tab. A port marked "Vibe Network
 * only" that nothing is enforcing is open to the internet, and that is not a
 * fact to learn by clicking a button that sounds like it changes something.
 *
 * The three answers are deliberately not "yes/no":
 *
 * - `public` - the port is meant to be reachable, so "unprotected" would be
 *   describing the intention as a fault.
 * - `protected` - the Node has a firewall, it is enforcing, and a rule for
 *   this port exists.
 * - `unprotected` - the port is not meant to be public and one of those three
 *   is missing. This is the case worth a red badge.
 * - `unknown` - the Node has not answered yet, or this is a local
 *   application with no Node firewall to ask about. Saying nothing is right;
 *   guessing "protected" here would be the dangerous guess.
 */
export type PortProtection = "public" | "protected" | "unprotected" | "unknown";

export function portProtection(port: ApplicationPort, overview: NodeFirewallOverview | null | undefined): PortProtection {
  // Bound to loopback, so nothing outside the Node can reach it whatever the
  // firewall is doing. The kernel refuses the connection at the socket - see
  // `resolve_bind_address`.
  if (port.visibility === "localhost") return "protected";
  if (port.visibility === "public") return "public";
  if (!overview) return "unknown";

  // No backend, or one that is installed and switched off: the rules exist
  // in VibeSSH and nothing on the Node is applying them.
  if (!overview.backend || !overview.active) return "unprotected";

  const covered = overview.rules.some((rule) => rule.port === effectivePort(port) && rule.protocol === port.protocol);
  if (!covered) return "unprotected";

  return containerProtection(port, overview);
}

/**
 * The second half of the answer, for a port Docker published.
 *
 * A ufw rule is not enough on its own. Docker writes its own DNAT and
 * ACCEPT ahead of ufw's chains, so a published port stays reachable however
 * correct `ufw status` looks - which is the whole reason
 * `firewall::docker_user` exists. Deciding "protected" from the ufw rule
 * alone told the user their database was closed while it was answering the
 * internet.
 *
 * Reading the chain can also simply fail, and that answer is `unknown`
 * rather than `protected`. The two are not close: one sends somebody to
 * look, the other sends them away satisfied.
 */
function containerProtection(port: ApplicationPort, overview: NodeFirewallOverview): PortProtection {
  const container = overview.container;
  // The Node runs no Docker, so nothing bypasses ufw here and its rule is
  // the whole story.
  if (!container?.applicable) return "protected";
  // Not published outside its container at all - there is no host port for
  // anyone to reach, restricted or otherwise.
  if (port.externalPort === null || port.externalPort === undefined) return "protected";
  if (container.error) return "unknown";

  // Either number is a real answer: after DNAT the chain sees the
  // container's own port, and a container that is not DNAT-ed (host
  // networking, or a port published out of band) is seen on the published
  // one. `docker_user` writes both for exactly this reason.
  const restricted = container.restrictedPorts.includes(port.internalPort) || container.restrictedPorts.includes(port.externalPort);
  return restricted ? "protected" : "unprotected";
}

/**
 * The port a firewall rule is written against.
 *
 * A published container port is reached from outside on its *external* port;
 * the internal one is what it listens on inside the container and is not
 * what anybody can connect to. Matching on the wrong one of these two is how
 * a rule that exists reads as missing.
 */
function effectivePort(port: ApplicationPort): number {
  return port.externalPort ?? port.internalPort;
}
