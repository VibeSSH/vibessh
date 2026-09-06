import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import type { ApplicationTemplate } from "@/types/applicationTemplate";
import type { Blueprint } from "@/types/application";

const listApplicationTemplates = vi.fn();
const listBlueprints = vi.fn();

vi.mock("@/services/applicationTemplateService", () => ({
  listApplicationTemplates: () => listApplicationTemplates(),
  saveApplicationTemplate: vi.fn(),
  deleteApplicationTemplate: vi.fn(),
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

vi.mock("@/services/databaseService", () => ({ listDatabaseHosts: () => Promise.resolve([]) }));
vi.mock("@/services/serverService", () => ({
  listServers: () => Promise.resolve([]),
  installDocker: vi.fn(),
  probeServerCapabilities: vi.fn(),
  serverSummaryToManagedServer: (server: unknown) => server,
}));
vi.mock("@tauri-apps/plugin-shell", () => ({ open: vi.fn() }));

const { CreateApplicationWizard } = await import("./CreateApplicationWizard");

function blueprint(id: string, name: string): Blueprint {
  return {
    id,
    name,
    description: "",
    schemaVersion: 1,
    blueprintVersion: 1,
    supportedRuntimeTypes: ["docker"],
    features: [],
    fields: [],
    knownFiles: [],
    isBuiltin: true,
  };
}

function template(id: string, name: string, blueprintId: string): ApplicationTemplate {
  return {
    id,
    name,
    blueprintId,
    runtimeType: "docker",
    fieldValues: {},
    environment: [],
    createdAt: "1970-01-01T00:00:00Z",
    isBuiltin: true,
  };
}

/**
 * Which template the wizard was filled in from.
 *
 * The list used to give no sign at all: clicking a row silently rewrote the
 * form behind it, so somebody who clicked one, scrolled, and came back had no
 * way to tell whether they had picked one - or which. What matters is that
 * the mark follows the answers rather than the last click, so it stops
 * claiming a template as soon as the form stops being that template.
 */
describe("CreateApplicationWizard templates", () => {
  beforeEach(() => {
    listApplicationTemplates.mockResolvedValue([
      template("11111111-1111-4111-8111-111111111111", "MariaDB", "mariadb"),
      template("22222222-2222-4222-8222-222222222222", "phpMyAdmin", "phpmyadmin"),
    ]);
    listBlueprints.mockResolvedValue([blueprint("mariadb", "MariaDB"), blueprint("phpmyadmin", "phpMyAdmin"), blueprint("paper", "Paper")]);
  });

  function renderWizard() {
    render(
      <MemoryRouter>
        <CreateApplicationWizard onClose={() => {}} onCreated={() => {}} />
      </MemoryRouter>,
    );
  }

  /** `aria-pressed` rather than the class, so this asserts what a screen
   * reader is told, not how it happens to be painted. */
  function templateRow(name: string) {
    return screen.getAllByRole("button").find((button) => button.textContent?.includes(name) && button.className.includes("wizard-template-use"));
  }

  it("marks nothing until a template is chosen", async () => {
    renderWizard();

    await waitFor(() => expect(templateRow("MariaDB")).toBeTruthy());

    expect(templateRow("MariaDB")).toHaveAttribute("aria-pressed", "false");
    expect(templateRow("phpMyAdmin")).toHaveAttribute("aria-pressed", "false");
  });

  it("marks the chosen one, and only that one", async () => {
    renderWizard();
    await waitFor(() => expect(templateRow("MariaDB")).toBeTruthy());

    await userEvent.click(templateRow("MariaDB")!);

    expect(templateRow("MariaDB")).toHaveAttribute("aria-pressed", "true");
    expect(templateRow("phpMyAdmin")).toHaveAttribute("aria-pressed", "false");
  });

  /**
   * The half that keeps the mark honest. Choosing an application type by
   * hand is the point where the form stops holding what the template said,
   * so a row still marked would be describing answers that are no longer
   * there.
   */
  it("drops the mark once the type is chosen by hand", async () => {
    renderWizard();
    await waitFor(() => expect(templateRow("MariaDB")).toBeTruthy());
    await userEvent.click(templateRow("MariaDB")!);

    // Step 1 will not let go without both of these. The inputs are wrapped
    // by their labels rather than linked by id, so they are addressed in
    // document order: name first, working directory second.
    const [nameInput, directoryInput] = screen.getAllByRole("textbox") as HTMLInputElement[];
    await userEvent.type(nameInput, "test");
    await userEvent.type(directoryInput, "/srv/test");
    await userEvent.click(screen.getByRole("button", { name: /next|dalej/i }));

    const paperTile = await waitFor(() => {
      const tile = screen.getAllByRole("button").find((button) => button.className.includes("wizard-blueprint-tile") && button.textContent?.includes("Paper"));
      expect(tile).toBeTruthy();
      return tile!;
    });
    await userEvent.click(paperTile);
    // Back to the first step, where the template list is.
    await userEvent.click(screen.getByRole("button", { name: /back|wstecz/i }));

    expect(templateRow("MariaDB")).toHaveAttribute("aria-pressed", "false");
  });

  it("moves the mark when a different template is chosen", async () => {
    renderWizard();
    await waitFor(() => expect(templateRow("MariaDB")).toBeTruthy());

    await userEvent.click(templateRow("MariaDB")!);
    await userEvent.click(templateRow("phpMyAdmin")!);

    expect(templateRow("MariaDB")).toHaveAttribute("aria-pressed", "false");
    expect(templateRow("phpMyAdmin")).toHaveAttribute("aria-pressed", "true");
  });
});
