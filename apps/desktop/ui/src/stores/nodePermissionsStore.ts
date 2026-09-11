import { create } from "zustand";
import { cloudListServers, cloudListTeams, cloudMyPermissions } from "@/services/cloudService";
import { useServersStore } from "./serversStore";

/**
 * What the signed-in user is allowed to do on servers their team shares.
 *
 * **Read this before trusting it.** These permissions are guard rails, not
 * a security boundary. The operations they cover - creating an
 * application, opening a port, changing a firewall rule, opening a
 * terminal - never touch the backend: the desktop app performs them over
 * its own SSH connection, with the operator's own credentials, from the
 * operator's own machine. So this can stop a colleague doing something by
 * accident, and cannot stop one who does not want to be stopped: anybody
 * who can reach the Node over SSH can do the same thing without VibeSSH at
 * all. The interface says so where the permissions are granted, and the
 * guide says so at length. Never present this as protection from a person.
 *
 * What it is genuinely good for is the thing that was asked for: a new
 * member does not see buttons they should not be pressing yet, and cannot
 * press one by mistake.
 */

/** A server the app knows locally, matched to a server a team shares. */
function serverKey(host: string, port: number): string {
  return `${host.trim().toLowerCase()}:${port}`;
}

interface NodePermissionsState {
  /** Team-shared servers, keyed by host and port, to the permissions the
   * signed-in user holds on them. */
  byServer: Record<string, string[]>;
  /** False until the first load resolves. Everything is permitted while
   * this is false - see `canOnServer`. */
  loaded: boolean;
  load: () => Promise<void>;
  clear: () => void;
}

export const useNodePermissionsStore = create<NodePermissionsState>((set) => ({
  byServer: {},
  loaded: false,

  /**
   * Rebuilds the map from every team the user belongs to.
   *
   * The union across teams, not the intersection: two teams sharing one
   * server and granting different things means the user may do either, the
   * same way holding two roles in one team does.
   */
  load: async () => {
    try {
      const teams = await cloudListTeams();
      const byServer: Record<string, string[]> = {};
      for (const team of teams) {
        const [servers, permissions] = await Promise.all([cloudListServers(team.id), cloudMyPermissions(team.id)]);
        for (const server of servers) {
          const key = serverKey(server.host, server.sshPort);
          byServer[key] = [...new Set([...(byServer[key] ?? []), ...permissions])];
        }
      }
      set({ byServer, loaded: true });
    } catch {
      // Signed out, offline, or a backend that does not answer. Leaving
      // `loaded` false is what keeps the app fully usable in all three -
      // see `canOnServer` for why that is the right failure.
      set({ byServer: {}, loaded: false });
    }
  },

  clear: () => set({ byServer: {}, loaded: false }),
}));

/**
 * Whether the current user may do `permission` on the server with this id.
 *
 * **Permitted by default**, and deliberately so. A server nobody shares is
 * the operator's own machine and none of a team's business; the app being
 * signed out, offline, or unable to reach the backend must not turn into a
 * locked interface on somebody's own servers. Restriction applies only
 * where there is a positive answer saying it should: this user, this
 * shared server, this permission absent.
 */
export function canOnServer(serverId: string | null | undefined, permission: string): boolean {
  const { byServer, loaded } = useNodePermissionsStore.getState();
  if (!loaded || !serverId) return true;

  const server = useServersStore.getState().servers.find((candidate) => candidate.id === serverId);
  if (!server) return true;

  const granted = byServer[serverKey(server.host, server.sshPort ?? 22)];
  // Not shared with any team - not a team's business.
  if (!granted) return true;
  return granted.includes(permission);
}

/** The same, as a hook, so a component re-renders when the map loads. */
export function useCanOnServer(serverId: string | null | undefined, permission: string): boolean {
  const byServer = useNodePermissionsStore((state) => state.byServer);
  const loaded = useNodePermissionsStore((state) => state.loaded);
  const server = useServersStore((state) => state.servers.find((candidate) => candidate.id === serverId));

  if (!loaded || !serverId || !server) return true;
  const granted = byServer[serverKey(server.host, server.sshPort ?? 22)];
  if (!granted) return true;
  return granted.includes(permission);
}

export const NODE_PERMISSION_KEY_FOR_TESTS = serverKey;
