export type PortForwardKind = "local" | "remote" | "dynamic";

export interface PortForwardStatus {
  id: string;
  serverId: string;
  kind: PortForwardKind;
  bindAddress: string;
  bindPort: number;
  targetHost: string | null;
  targetPort: number | null;
}

export interface StartPortForwardInput {
  serverId: string;
  kind: PortForwardKind;
  bindAddress: string;
  /** `0` asks the OS/Node for any free port. */
  bindPort: number;
  /** Required for local/remote, ignored for dynamic. */
  targetHost?: string;
  targetPort?: number;
}
