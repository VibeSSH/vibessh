/**
 * Sample data, for looking at the interface without a Node.
 *
 * Two uses, both real. It is how the guide's screenshots are taken - a
 * manual should show a populated screen, and it must not show somebody's
 * actual servers, addresses or application names. And it is how a change to
 * a screen can be seen at all when there is no VPS to hand.
 *
 * **This never reaches a build.** It is imported dynamically behind
 * `import.meta.env.DEV`, so the production bundle does not contain it, and
 * even in development it does nothing unless the URL asks for it with
 * `?fixtures=1`. There is no setting, no toggle and no state that could
 * leave it on by accident.
 *
 * It works by standing in for Tauri's own bridge. `@tauri-apps/api` calls
 * `window.__TAURI_INTERNALS__.invoke`, which does not exist in a plain
 * browser - which is why the app shows empty screens there. Defining it
 * before React mounts is enough for every service call to be answered.
 *
 * A command with no fixture rejects rather than resolving with something
 * empty: a screen that quietly renders "none" would be a screenshot of a
 * lie, while a rejection shows up as the error state it really is.
 */

import { useServersStore } from "@/stores/serversStore";

const SERVER_ID = "9f1c2f7a-4a1d-4c1e-9a2b-6d7e8f901234";
const SECOND_SERVER_ID = "1b2c3d4e-5f60-4718-8293-a4b5c6d7e8f9";
const APPLICATION_ID = "3a7d5e21-9c64-4f0b-8d31-2e5a6b7c8d90";

const application = {
  id: APPLICATION_ID,
  name: "paper",
  blueprintId: "paper",
  blueprintName: "Paper",
  serverId: SERVER_ID,
  runtimeType: "docker",
  status: "running",
  workingDirectory: "/srv/vibessh/paper",
  createdAt: "2026-08-14T09:12:00Z",
};

const blueprint = {
  id: "paper",
  name: "Paper",
  description: "Serwer Minecraft",
  category: "minecraft",
  icon: "box",
  fields: [],
  knownFiles: ["server.properties", "spigot.yml", "paper-global.yml"],
  features: ["console", "files", "logs", "ports", "databases", "backups"],
};

const servers = [
  {
    id: SERVER_ID,
    name: "vps",
    host: "203.0.113.10",
    port: 22,
    username: "root",
    connectionMode: "ssh",
    icon: null,
    nodeCapabilities: { docker: true, wireguard: true },
  },
  {
    id: SECOND_SERVER_ID,
    name: "skytop",
    host: "198.51.100.24",
    port: 22,
    username: "root",
    connectionMode: "ssh",
    icon: null,
    nodeCapabilities: { docker: true, wireguard: true },
  },
];

/** Enough log to fill a console, with one line of each severity so the
 * colouring is visible in a screenshot. */
const logLines = [
  "[12:04:01 INFO]: Starting minecraft server version 1.21.4",
  "[12:04:02 INFO]: Loading properties",
  "[12:04:03 INFO]: Default game type: SURVIVAL",
  "[12:04:07 INFO]: Preparing level \"world\"",
  "[12:04:11 WARN]: Plugin `EssentialsX` is not marked as compatible with this version",
  "[12:04:12 INFO]: Preparing spawn area: 84%",
  "[12:04:15 INFO]: Done (13.204s)! For help, type \"help\"",
  "[12:07:32 INFO]: Notch joined the game",
  "[12:09:18 ERROR]: Could not pass event PlayerJoinEvent to ExamplePlugin v1.2",
  "[12:09:18 INFO]: Notch left the game",
];

const fixtures: Record<string, unknown> = {
  list_servers: servers,
  list_applications: [application],
  // `ApplicationDetail` is the list shape plus four fields the detail page
  // reads directly; missing any of them is a blank page rather than an
  // empty one, so they are all here.
  get_application: {
    ...application,
    environment: [
      { key: "MEMORY", value: "4G", secret: false },
      { key: "EULA", value: "TRUE", secret: false },
      { key: "RCON_PASSWORD", value: "********", secret: true },
    ],
    ports: [
      { id: "port-1", name: "Minecraft", protocol: "tcp", bindAddress: "0.0.0.0", internalPort: 25565, externalPort: null, visibility: "public", required: true },
      { id: "port-2", name: "RCON", protocol: "tcp", bindAddress: "0.0.0.0", internalPort: 25575, externalPort: null, visibility: "vibeNetwork", required: false },
    ],
    links: [],
    runtimeConfig: { image: "itzg/minecraft-server:latest" },
    metadata: {},
  },
  list_blueprints: [blueprint],
  refresh_application_status: "running",
  get_application_resource_usage: { cpuPercent: 38.4, ramBytes: 3_355_443_200, uptimeSeconds: 9_240 },
  get_application_logs: logLines,
  get_application_health: null,

  list_application_ports: [
    { id: "port-1", name: "Minecraft", protocol: "tcp", bindAddress: "0.0.0.0", internalPort: 25565, externalPort: null, visibility: "public", required: true },
    { id: "port-2", name: "RCON", protocol: "tcp", bindAddress: "0.0.0.0", internalPort: 25575, externalPort: null, visibility: "vibeNetwork", required: false },
    { id: "port-3", name: "Metrics", protocol: "tcp", bindAddress: "127.0.0.1", internalPort: 9100, externalPort: 9101, visibility: "localhost", required: false },
  ],
  list_application_links: [],

  list_application_files: [
    { name: "cache", path: "cache", isDir: true, isSymlink: false, size: 0, modifiedAt: "2026-08-30T18:20:00Z", permissions: 0o755 },
    { name: "plugins", path: "plugins", isDir: true, isSymlink: false, size: 0, modifiedAt: "2026-09-01T11:02:00Z", permissions: 0o755 },
    { name: "world", path: "world", isDir: true, isSymlink: false, size: 0, modifiedAt: "2026-09-02T12:09:00Z", permissions: 0o755 },
    { name: "eula.txt", path: "eula.txt", isDir: false, isSymlink: false, size: 178, modifiedAt: "2026-08-14T09:12:00Z", permissions: 0o644 },
    { name: "paper-global.yml", path: "paper-global.yml", isDir: false, isSymlink: false, size: 12_486, modifiedAt: "2026-08-28T22:41:00Z", permissions: 0o644 },
    { name: "server.properties", path: "server.properties", isDir: false, isSymlink: false, size: 1_374, modifiedAt: "2026-09-01T10:55:00Z", permissions: 0o644 },
    { name: "spigot.yml", path: "spigot.yml", isDir: false, isSymlink: false, size: 6_902, modifiedAt: "2026-08-28T22:41:00Z", permissions: 0o644 },
  ],

  list_application_backups: [
    { id: "backup-1", createdAt: "2026-09-02T03:00:00Z", sizeBytes: 812_000_000, kind: "scheduled", uploadedToDestination: true },
    { id: "backup-2", createdAt: "2026-09-01T03:00:00Z", sizeBytes: 806_400_000, kind: "scheduled", uploadedToDestination: true },
    { id: "backup-3", createdAt: "2026-08-30T19:14:00Z", sizeBytes: 794_900_000, kind: "manual", uploadedToDestination: false },
  ],
  get_application_backup_schedule: { enabled: true, intervalHours: 24, keepLast: 7, maxAgeDays: 30, maxTotalMb: 20_480 },

  list_application_databases: [
    { id: "db-1", databaseHostId: "host-1", databaseName: "paper_luckperms", username: "paper_lp", purpose: "luckperms" },
  ],
  list_database_hosts: [{ id: "host-1", name: "MariaDB (vps)", host: "127.0.0.1", port: 3306, username: "root", phpmyadminUrl: null }],

  list_network_members: [
    { serverId: SERVER_ID, wireguardIp: "10.77.0.1", wireguardPublicKey: "kPd1...=", endpoint: "203.0.113.10:54221" },
    { serverId: SECOND_SERVER_ID, wireguardIp: "10.77.0.2", wireguardPublicKey: "b3Fq...=", endpoint: "198.51.100.24:54221" },
  ],
  get_vibe_network_status: [
    {
      serverId: SERVER_ID,
      reachable: true,
      tunnel: "up",
      tunnelError: null,
      unknownPeers: 0,
      peers: [{ serverId: SECOND_SERVER_ID, latestHandshakeUnix: Math.floor(Date.now() / 1000) - 34, rxBytes: 8_421_000, txBytes: 6_118_000 }],
    },
    {
      serverId: SECOND_SERVER_ID,
      reachable: true,
      tunnel: "up",
      tunnelError: null,
      unknownPeers: 0,
      peers: [{ serverId: SERVER_ID, latestHandshakeUnix: Math.floor(Date.now() / 1000) - 51, rxBytes: 6_118_000, txBytes: 8_421_000 }],
    },
  ],
  resolve_dns_view: [
    { serverId: SERVER_ID, name: "vps.vibe", address: "10.77.0.1", kind: "node" },
    { serverId: SECOND_SERVER_ID, name: "skytop.vibe", address: "10.77.0.2", kind: "node" },
  ],
  list_dns_records: [],
  list_node_endpoints: [],

  get_node_firewall_overview: {
    backend: "ufw",
    active: true,
    rules: [
      { id: "fw-1", port: 22, protocol: "tcp", origin: "ssh", label: null, sourceCidr: null },
      { id: "fw-2", port: 54221, protocol: "udp", origin: "wireguard", label: null, sourceCidr: null },
      { id: "fw-3", port: 25565, protocol: "tcp", origin: "application", label: "paper", sourceCidr: null },
      { id: "fw-4", port: 25575, protocol: "tcp", origin: "application", label: "paper", sourceCidr: "10.77.0.0/16" },
    ],
  },
  list_port_forwards: [],

  list_server_processes: [
    { pid: 1421, user: "vibessh-app", cpuPercent: 41.2, ramBytes: 3_355_443_200, command: "java -Xms4G -Xmx4G -jar paper.jar nogui" },
    { pid: 918, user: "mysql", cpuPercent: 1.4, ramBytes: 486_539_264, command: "/usr/sbin/mariadbd" },
    { pid: 640, user: "root", cpuPercent: 0.6, ramBytes: 121_634_816, command: "/usr/bin/dockerd -H fd://" },
    { pid: 1, user: "root", cpuPercent: 0.0, ramBytes: 12_582_912, command: "/sbin/init" },
  ],
  list_server_services: [
    { name: "docker.service", description: "Docker Application Container Engine", active: true, enabled: true },
    { name: "ssh.service", description: "OpenBSD Secure Shell server", active: true, enabled: true },
    { name: "mariadb.service", description: "MariaDB database server", active: true, enabled: true },
    { name: "unattended-upgrades.service", description: "Unattended Upgrades Shutdown", active: false, enabled: true },
  ],
  list_server_containers: [
    { id: "c1f0", name: "vibessh-paper", image: "itzg/minecraft-server:latest", running: true, status: "Up 2 hours" },
    { id: "a93b", name: "vibessh-velocity", image: "itzg/bungeecord:latest", running: false, status: "Exited (0) 3 days ago" },
  ],

  // Field for field with `ServerMetrics` - a near miss here is not a
  // compile error, it is a page that says "collecting..." forever.
  get_server_metrics: {
    cpuUsagePercent: 22.8,
    ramUsedBytes: 5_100_273_664,
    ramTotalBytes: 16_642_998_272,
    diskUsedBytes: 41_875_931_136,
    diskTotalBytes: 107_374_182_400,
    loadAverage1m: 0.82,
    uptimeSeconds: 1_209_600,
    networkRxBytesPerSec: 184_320,
    networkTxBytesPerSec: 96_256,
  },
  get_node_sync_status: { serverId: SERVER_ID, desiredRevision: 12, appliedRevision: 12, inSync: true },
  ping_server: 6,

  // The assistant and the cloud are deliberately inert here: a screenshot
  // must never depend on a real endpoint answering.
  get_ai_config: { enabled: false, provider: "openAiCompatible", baseUrl: "", model: "", hasApiKey: false },
  get_ai_quota: null,
  // Signed in, with one team, so the Teams screens can be looked at and
  // photographed like every other part of the app.
  cloud_session_info: { userId: "user-1", email: "ty@example.com", displayName: "Ty" },
  cloud_list_teams: [{ id: "team-1", name: "test", ownerId: "user-1", createdAt: "2026-08-20T09:00:00Z" }],
  cloud_get_team: { id: "team-1", name: "test", ownerId: "user-1", createdAt: "2026-08-20T09:00:00Z" },
  cloud_list_permissions: [
    "applications.view",
    "applications.create",
    "applications.lifecycle",
    "applications.delete",
    "applications.config",
    "applications.ports",
    "applications.files.read",
    "applications.files.write",
    "applications.backups",
    "applications.databases",
    "node.terminal",
    "node.firewall",
    "node.services",
    "node.software",
    "node.network",
    "team.view",
    "team.update",
    "team.delete",
    "team.members.add",
    "team.members.remove",
    "team.invitations.manage",
    "team.roles.manage",
    "team.roles.assign",
    "audit.view",
    "servers.manage",
  ],
  cloud_list_roles: [
    {
      id: "role-owner",
      teamId: "team-1",
      name: "Owner",
      description: "Full control over the team - every permission, cannot be edited or deleted.",
      isSystem: true,
      permissions: [
        "team.view",
        "team.update",
        "team.delete",
        "team.members.add",
        "team.members.remove",
        "team.invitations.manage",
        "team.roles.manage",
        "team.roles.assign",
        "audit.view",
        "servers.manage",
      ],
    },
    {
      id: "role-ops",
      teamId: "team-1",
      name: "Operator",
      description: "Prowadzi serwery na co dzień, bez prawa zmieniania ról.",
      isSystem: false,
      permissions: ["team.view", "servers.manage", "audit.view", "team.roles.assign"],
    },
  ],
  cloud_my_permissions: [
    "team.view",
    "team.update",
    "team.delete",
    "team.members.add",
    "team.members.remove",
    "team.invitations.manage",
    "team.roles.manage",
    "team.roles.assign",
    "audit.view",
    "servers.manage",
  ],
  cloud_list_members: [{ userId: "user-1", email: "ty@example.com", displayName: "Ty", joinedAt: "2026-08-20T09:00:00Z" }],
  cloud_list_member_roles: [],
  cloud_list_invitations: [],
  cloud_list_servers: [],
  cloud_list_audit_events: [],
  list_registry_credentials: [],
  get_backup_destination: { enabled: false, endpoint: "", region: "", bucket: "", accessKeyId: "", pathPrefix: "", pathStyle: true, hasSecretKey: false },
  get_dns_suffix: "vibe",
  get_app_info: { version: "0.1.0" },
};

export function installDevFixtures(): void {
  // Landing straight on a deep link leaves the servers store empty, because
  // it is filled by the pages that list servers rather than at startup - so
  // a Node shows as its raw id. Seeding it makes every screenshot say
  // "vps" where a screen says which Node something runs on.
  useServersStore.getState().setServers(servers as never);

  const globals = window as unknown as { __TAURI_INTERNALS__?: unknown };
  globals.__TAURI_INTERNALS__ = {
    invoke: (command: string) =>
      Object.prototype.hasOwnProperty.call(fixtures, command)
        ? Promise.resolve(fixtures[command])
        : Promise.reject({ kind: "internal", code: "internal", params: null, message: `no fixture for ${command}` }),
    // Events never fire under fixtures - a console fed by a stream would
    // need a fake Node to stream from. The polled path is what shows.
    transformCallback: (callback: unknown) => {
      const id = Math.floor(Math.random() * 1e9);
      (window as unknown as Record<string, unknown>)[`_${id}`] = callback;
      return id;
    },
  };
}
