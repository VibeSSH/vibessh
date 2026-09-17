import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import type { Blueprint } from "@/types/application";

/**
 * Why the wizard will not let you continue.
 *
 * A disabled "Next" is not an explanation, and this wizard had two ways of
 * producing one with nothing on screen to read. The working directory is
 * suggested from wherever VibeSSH keeps application files on this computer,
 * and the call that asks for that discarded its own failure - so when it
 * failed the field stayed empty, "Next" went grey, and no message existed
 * anywhere in either language to say what was wrong. The same dead end waits
 * on step three for anyone who scrolls past a required blueprint field.
 */

const localApplicationsRoot = vi.fn();
const listBlueprints = vi.fn();

vi.mock("@/services/appService", () => ({
  localApplicationsRoot: () => localApplicationsRoot(),
  localDockerAvailable: () => Promise.resolve(true),
}));

vi.mock("@/services/applicationService", () => ({
  listBlueprints: () => listBlueprints(),
  listApplications: () => Promise.resolve([]),
  createApplication: vi.fn(),
  detectJavaInstallations: () => Promise.resolve([]),
  listPaperVersions: () => Promise.resolve([]),
  listPurpurVersions: () => Promise.resolve([]),
  listVelocityVersions: () => Promise.resolve([]),
  listWaterfallVersions: () => Promise.resolve([]),
}));

vi.mock("@/services/applicationTemplateService", () => ({
  listApplicationTemplates: () => Promise.resolve([]),
  saveApplicationTemplate: vi.fn(),
  deleteApplicationTemplate: vi.fn(),
}));

vi.mock("@/services/databaseService", () => ({ listDatabaseHosts: () => Promise.resolve([]) }));
vi.mock("@/services/serverService", () => ({
  listServers: () => Promise.resolve([]),
  installDocker: vi.fn(),
  probeServerCapabilities: vi.fn(),
  serverSummaryToManagedServer: (server: unknown) => server,
}));
vi.mock("@tauri-apps/plugin-shell", () => ({ open: vi.fn() }));

const { CreateApplicationWizard } = await import("./CreateApplicationWizard");

/** A blueprint with one required field, which is all step three needs. */
function blueprint(): Blueprint {
  return {
    id: "generic",
    name: "Generic",
    description: "",
    schemaVersion: 1,
    blueprintVersion: 1,
    supportedRuntimeTypes: ["docker"],
    features: [],
    fields: [{ key: "command", label: "Command", fieldType: "path", required: true }],
    knownFiles: [],
    isBuiltin: true,
  };
}

function open() {
  render(
    <MemoryRouter>
      <CreateApplicationWizard onClose={() => {}} onCreated={() => {}} />
    </MemoryRouter>,
  );
}

describe("CreateApplicationWizard, when it will not continue", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    localApplicationsRoot.mockResolvedValue("C:\\Users\\kompu\\AppData\\Roaming\\com.vibessh.app\\applications");
    listBlueprints.mockResolvedValue([blueprint()]);
  });

  /// The case that started this: the suggestion cannot be made, so the field
  /// is empty, so the button is dead. The reason was thrown away by a
  /// `.catch(() => undefined)` and the user was left with a grey button.
  it("says so when it cannot work out where to keep the files", async () => {
    localApplicationsRoot.mockRejectedValue(new Error("couldn't resolve the application data directory: access denied"));
    open();

    const note = await screen.findByText(/couldn't work out where it keeps application files/i);
    // The underlying reason travels with it - "it failed" sends nobody
    // anywhere, "access denied" sends them to the folder's permissions.
    expect(note.textContent).toContain("access denied");
  });

  /// And nothing is said when there is nothing wrong. A warning that is
  /// always on screen is one nobody reads on the day it matters.
  it("says nothing about the directory when the suggestion worked", async () => {
    open();

    await screen.findByDisplayValue(/com\.vibessh\.app/);
    expect(screen.queryByText(/couldn't work out where it keeps application files/i)).toBeNull();
  });

  /// The empty answers are named, rather than left for the reader to hunt.
  it("names both missing answers on the first step, and stops naming each as it is filled in", async () => {
    localApplicationsRoot.mockRejectedValue(new Error("no application data directory"));
    open();

    const blocked = await screen.findByText(/Before you can continue, fill in:/);
    expect(blocked.textContent).toContain("Name");
    expect(blocked.textContent).toContain("Working directory");

    await userEvent.type(screen.getByPlaceholderText("My application"), "paper");
    expect(screen.getByText(/Before you can continue, fill in:/).textContent).not.toContain("Name");
    expect(screen.getByText(/Before you can continue, fill in:/).textContent).toContain("Working directory");
  });

  /// Nothing left to fill in means nothing to say - the button being
  /// available is the whole message.
  it("says nothing once the step is complete", async () => {
    open();

    await screen.findByDisplayValue(/com\.vibessh\.app/);
    await userEvent.type(screen.getByPlaceholderText("My application"), "paper");

    expect(screen.queryByText(/Before you can continue, fill in:/)).toBeNull();
    expect(screen.getByRole("button", { name: "Next" })).toBeEnabled();
  });

  /// A required blueprint field is the other way to reach a dead button, and
  /// the worse one: on a long blueprint the empty field can be scrolled out
  /// of sight entirely. It is named by the label it carries on screen.
  it("names a required blueprint field left empty on the configuration step", async () => {
    open();

    await screen.findByDisplayValue(/com\.vibessh\.app/);
    await userEvent.type(screen.getByPlaceholderText("My application"), "paper");
    await userEvent.click(screen.getByRole("button", { name: "Next" }));
    await userEvent.click(await screen.findByText("Generic"));
    await userEvent.click(screen.getByRole("button", { name: "Next" }));

    const blocked = await screen.findByText(/Before you can continue, fill in:/);
    expect(blocked.textContent).toContain("Command");
  });
});
