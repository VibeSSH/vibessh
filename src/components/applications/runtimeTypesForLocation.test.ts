import { describe, expect, it } from "vitest";
import { runtimeTypesForLocation } from "./CreateApplicationWizard";
import type { Blueprint, RuntimeType } from "@/types/application";

function blueprint(supportedRuntimeTypes: RuntimeType[]): Blueprint {
  return {
    id: "paper",
    name: "Paper",
    description: "",
    schemaVersion: 1,
    blueprintVersion: 1,
    supportedRuntimeTypes,
    features: [],
    fields: [],
    knownFiles: [],
    isBuiltin: true,
  };
}

/**
 * Which runtimes a blueprint offers, and in what order.
 *
 * The order is the point on a local machine. Blueprints list Docker first
 * because that is what they were written for, so the option somebody reached
 * for on Windows was the one needing Docker Desktop, WSL2 and a restart -
 * while the other one needs nothing and downloads its own Java.
 */
describe("runtimeTypesForLocation", () => {
  const paper = blueprint(["docker", "localProcess"]);

  it("offers the plain process first on this computer", () => {
    expect(runtimeTypesForLocation(paper, true)).toEqual(["localProcess", "docker"]);
  });

  it("keeps Docker available locally rather than hiding it", () => {
    expect(runtimeTypesForLocation(paper, true)).toContain("docker");
  });

  /** A Node has no local process to run, and the blueprint's own order is
   * the right one there - nothing about this reordering should reach it. */
  it("leaves a node's options as the blueprint wrote them", () => {
    expect(runtimeTypesForLocation(blueprint(["docker", "localProcess"]), false)).toEqual(["docker"]);
    expect(runtimeTypesForLocation(blueprint(["localProcess", "remoteProcess", "systemd"]), false)).toEqual(["remoteProcess", "systemd"]);
  });

  it("offers only Docker locally for a blueprint that has nothing else", () => {
    expect(runtimeTypesForLocation(blueprint(["docker"]), true)).toEqual(["docker"]);
  });

  it("offers only the process locally for a blueprint with no Docker", () => {
    expect(runtimeTypesForLocation(blueprint(["localProcess", "remoteProcess", "systemd"]), true)).toEqual(["localProcess"]);
  });
});
