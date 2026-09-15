import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ServersSection } from "./ServersSection";

/**
 * The screen where "removed from the team" and "removed from the machine"
 * are two different facts.
 *
 * Everything here is about the second one being told truthfully. A list that
 * showed somebody as gone while their key is still in an `authorized_keys`
 * file would be worse than no list at all, and it is the one thing this
 * component exists to prevent.
 */

const cloudListServers = vi.fn();
const listPendingRevocations = vi.fn();
const syncTeamNodeAccess = vi.fn();
const listServers = vi.fn();

vi.mock("@/services/cloudService", () => ({
  cloudListServers: (...args: unknown[]) => cloudListServers(...args),
  cloudCreateServer: vi.fn(),
  cloudDeleteServer: vi.fn(),
  listPendingRevocations: (...args: unknown[]) => listPendingRevocations(...args),
  syncTeamNodeAccess: (...args: unknown[]) => syncTeamNodeAccess(...args),
}));

vi.mock("@/services/serverService", () => ({
  listServers: (...args: unknown[]) => listServers(...args),
}));

vi.mock("@/stores/toastStore", () => ({ toastSuccess: vi.fn() }));

vi.mock("react-i18next", () => ({
  // The assertions are about which message is shown and with what in it, so
  // the key and its values are rendered rather than the translated sentence.
  // A test that matched Polish prose would fail the next time somebody
  // improved the wording, which is not the behaviour worth protecting.
  useTranslation: () => ({
    t: (key: string, values?: Record<string, unknown>) => (values ? `${key} ${JSON.stringify(values)}` : key),
  }),
}));

const TEAM_ID = "team-1";
const SERVER = { id: "team-server-1", teamId: TEAM_ID, name: "Prod", host: "10.0.0.1", sshPort: 22, username: "root", createdAt: "" };
const LOCAL = { id: "local-1", name: "Prod", host: "10.0.0.1", sshPort: 22, username: "root" };

const REVOCATION = {
  id: "revocation-1",
  teamServerId: "team-server-1",
  serverName: "Prod",
  host: "10.0.0.1",
  sshPort: 22,
  userId: "user-9",
  nodeUsername: "vibessh-m-0123456789ab",
  email: "left@example.com",
  requestedAt: "2026-09-13T10:00:00Z",
};

describe("ServersSection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    cloudListServers.mockResolvedValue([SERVER]);
    listPendingRevocations.mockResolvedValue([]);
    listServers.mockResolvedValue([LOCAL]);
  });

  /// Shown on arrival, not only after somebody presses sync. Whoever opens
  /// this screen needs to know the machine still has an account for a person
  /// the team removed, whether or not they have done anything today.
  it("warns that a removed member is still on the Node, before anything is pressed", async () => {
    listPendingRevocations.mockResolvedValue([REVOCATION]);
    render(<ServersSection teamId={TEAM_ID} canManage />);

    const warning = await screen.findByText(/teamServers\.revocationPending/);
    expect(warning.textContent).toContain("left@example.com");
    expect(warning.textContent).toContain("vibessh-m-0123456789ab");
    expect(syncTeamNodeAccess).not.toHaveBeenCalled();
  });

  /// The warning belongs to one machine. A pending revocation for another
  /// Node is not this Node's problem and saying so here would send somebody
  /// to the wrong server.
  it("does not show another Node's pending revocation", async () => {
    listPendingRevocations.mockResolvedValue([{ ...REVOCATION, teamServerId: "team-server-2", serverName: "Staging" }]);
    render(<ServersSection teamId={TEAM_ID} canManage />);

    await screen.findByText("Prod");
    expect(screen.queryByText(/teamServers\.revocationPending/)).toBeNull();
  });

  /// The install that removed somebody is usually not the one that can reach
  /// the machine. Saying which is what turns the warning into something the
  /// reader can act on rather than an alarm with no address.
  it("says so when this machine cannot reach the Node that still has the account", async () => {
    listServers.mockResolvedValue([]);
    listPendingRevocations.mockResolvedValue([REVOCATION]);
    render(<ServersSection teamId={TEAM_ID} canManage />);

    await screen.findByText(/teamServers\.revocationNoLocal/);
  });

  /// The sync is one operation over the whole desired state, and it has to
  /// name the Node it is for - the list covers every machine the team
  /// shares, and this install may reach exactly one of them.
  it("syncs the local server against this team server", async () => {
    syncTeamNodeAccess.mockResolvedValue({ members: [], revocations: [] });
    render(<ServersSection teamId={TEAM_ID} canManage />);

    await userEvent.click(await screen.findByRole("button", { name: /teamServers\.sync/ }));
    expect(syncTeamNodeAccess).toHaveBeenCalledWith("local-1", TEAM_ID, "team-server-1");
  });

  /// The name is how you tell which server a result belongs to, so it is the
  /// one thing that must survive the results appearing. It did not: the row
  /// is a flex line that does not wrap, the results are a full-width block,
  /// and the name was squeezed to nothing the first time a sync ran.
  it("keeps the server's name visible once the sync results are shown", async () => {
    syncTeamNodeAccess.mockResolvedValue({
      members: [{ userId: "u1", email: "someone@example.com", nodeUsername: "vibessh-m-0123456789ab", hasKey: true, granted: true, error: null }],
      revocations: [],
    });
    render(<ServersSection teamId={TEAM_ID} canManage />);

    await userEvent.click(await screen.findByRole("button", { name: /teamServers\.sync/ }));
    await screen.findByText(/teamServers\.grantOk/);

    // Still on screen, and on a row that is allowed to wrap rather than to
    // crush its first column.
    const name = screen.getByText("Prod");
    expect(name).toBeTruthy();
    expect(name.closest("li")?.className).toContain("team-servers-row");
  });

  /// A revocation that failed is still owed, and the list is re-read from
  /// the backend rather than adjusted here. Guessing locally is how a person
  /// whose key is still in place ends up shown as removed.
  it("re-reads what is owed after a sync instead of assuming it worked", async () => {
    listPendingRevocations.mockResolvedValueOnce([REVOCATION]).mockResolvedValueOnce([REVOCATION]);
    syncTeamNodeAccess.mockResolvedValue({
      members: [],
      revocations: [{ id: REVOCATION.id, email: REVOCATION.email, nodeUsername: REVOCATION.nodeUsername, completed: false, error: "no route to host" }],
    });
    render(<ServersSection teamId={TEAM_ID} canManage />);

    await userEvent.click(await screen.findByRole("button", { name: /teamServers\.sync/ }));

    await waitFor(() => expect(listPendingRevocations).toHaveBeenCalledTimes(2));
    const failure = await screen.findByText(/teamServers\.revocationFailed/);
    expect(failure.textContent).toContain("no route to host");
    // And the warning is still there, because it is still true.
    expect(screen.getByText(/teamServers\.revocationPending/)).toBeTruthy();
  });
});
