import { callCommand } from "./tauri";
import type { PortForwardStatus, StartPortForwardInput } from "@/types/portForward";

/** Starts an `ssh -L`/`-R`/`-D`-shaped tunnel against a saved server - purely in-memory, ends when the app closes or `stopPortForward` is called. */
export function startPortForward(input: StartPortForwardInput): Promise<PortForwardStatus> {
  return callCommand<PortForwardStatus>("start_port_forward", { input });
}

/** Every tunnel currently open, across every server. */
export function listPortForwards(): Promise<PortForwardStatus[]> {
  return callCommand<PortForwardStatus[]>("list_port_forwards");
}

export function stopPortForward(id: string): Promise<void> {
  return callCommand<void>("stop_port_forward", { id });
}
