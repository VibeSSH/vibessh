import { beforeEach, describe, expect, it } from "vitest";
import { useApplicationsStore } from "./applicationsStore";
import type { Application } from "@/types/application";

function application(overrides: Partial<Application> = {}): Application {
  return {
    id: "app-1",
    serverId: "server-1",
    name: "Survival",
    description: null,
    blueprintId: "paper",
    blueprintVersion: 1,
    runtimeType: "docker",
    workingDirectory: "/srv/survival",
    status: "stopped",
    lastStatusCheckAt: null,
    healthCheckType: "process",
    healthCheckPortId: null,
    healthCheckHttpPath: null,
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
    ...overrides,
  } as Application;
}

describe("applicationsStore", () => {
  beforeEach(() => {
    useApplicationsStore.setState({ applications: [] });
  });

  it("replaces the whole list on setApplications", () => {
    useApplicationsStore.getState().setApplications([application(), application({ id: "app-2" })]);
    useApplicationsStore.getState().setApplications([application({ id: "app-3" })]);

    expect(useApplicationsStore.getState().applications.map((a) => a.id)).toEqual(["app-3"]);
  });

  it("appends an application it has not seen before", () => {
    useApplicationsStore.getState().upsertApplication(application());
    useApplicationsStore.getState().upsertApplication(application({ id: "app-2", name: "Creative" }));

    expect(useApplicationsStore.getState().applications).toHaveLength(2);
  });

  /**
   * Upsert merges rather than replaces, which is what lets a partial update
   * (a status poll, a rename) leave every other field alone. A replace here
   * would silently blank whatever the caller did not include.
   */
  it("merges into an existing application instead of replacing it", () => {
    useApplicationsStore.getState().setApplications([application({ name: "Survival", description: "the main one" })]);

    useApplicationsStore.getState().upsertApplication({ id: "app-1", name: "Renamed" } as Application);

    const [updated] = useApplicationsStore.getState().applications;
    expect(updated.name).toBe("Renamed");
    expect(updated.description).toBe("the main one");
    expect(updated.workingDirectory).toBe("/srv/survival");
  });

  it("keeps list order when upserting an existing application", () => {
    useApplicationsStore.getState().setApplications([application({ id: "a" }), application({ id: "b" }), application({ id: "c" })]);

    useApplicationsStore.getState().upsertApplication(application({ id: "b", name: "Changed" }));

    expect(useApplicationsStore.getState().applications.map((a) => a.id)).toEqual(["a", "b", "c"]);
  });

  it("updates only the targeted application's status", () => {
    useApplicationsStore.getState().setApplications([application({ id: "a" }), application({ id: "b" })]);

    useApplicationsStore.getState().updateStatus("a", "running");

    const byId = Object.fromEntries(useApplicationsStore.getState().applications.map((a) => [a.id, a.status]));
    expect(byId).toEqual({ a: "running", b: "stopped" });
  });

  it("ignores a status update for an application it does not have", () => {
    useApplicationsStore.getState().setApplications([application({ id: "a" })]);

    useApplicationsStore.getState().updateStatus("does-not-exist", "running");

    expect(useApplicationsStore.getState().applications).toHaveLength(1);
    expect(useApplicationsStore.getState().applications[0].status).toBe("stopped");
  });

  it("removes the application asked for and leaves the rest", () => {
    useApplicationsStore.getState().setApplications([application({ id: "a" }), application({ id: "b" })]);

    useApplicationsStore.getState().removeApplication("a");

    expect(useApplicationsStore.getState().applications.map((app) => app.id)).toEqual(["b"]);
  });

  it("removing an unknown application is a no-op, not an error", () => {
    useApplicationsStore.getState().setApplications([application({ id: "a" })]);

    useApplicationsStore.getState().removeApplication("does-not-exist");

    expect(useApplicationsStore.getState().applications).toHaveLength(1);
  });
});
