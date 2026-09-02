import { beforeEach, describe, expect, it } from "vitest";
import { canOnServer, useNodePermissionsStore } from "./nodePermissionsStore";
import { useServersStore } from "./serversStore";

const SERVER = {
  id: "server-1",
  name: "vps",
  host: "203.0.113.10",
  sshPort: 22,
  connectionMode: "ssh" as const,
  status: "online" as const,
};

const OTHER = { ...SERVER, id: "server-2", name: "own", host: "198.51.100.7" };

beforeEach(() => {
  useServersStore.setState({ servers: [SERVER, OTHER] });
  useNodePermissionsStore.setState({ byServer: {}, loaded: false });
});

describe("canOnServer", () => {
  // The failure that matters: nothing loaded must not mean nothing allowed.
  // Signed out, offline, or a backend that will not answer are all this
  // state, and none of them should lock somebody out of their own machines.
  it("permits everything before anything has loaded", () => {
    expect(canOnServer("server-1", "applications.create")).toBe(true);
  });

  it("permits everything on a server no team shares", () => {
    useNodePermissionsStore.setState({ byServer: { "203.0.113.10:22": ["applications.view"] }, loaded: true });
    expect(canOnServer("server-2", "applications.create")).toBe(true);
  });

  it("restricts a shared server to what the user was granted", () => {
    useNodePermissionsStore.setState({ byServer: { "203.0.113.10:22": ["applications.view"] }, loaded: true });
    expect(canOnServer("server-1", "applications.view")).toBe(true);
    expect(canOnServer("server-1", "applications.create")).toBe(false);
  });

  // A shared server with an empty grant is a real answer - "this team gives
  // you nothing here" - and must not be read as "not shared".
  it("restricts a shared server that grants nothing", () => {
    useNodePermissionsStore.setState({ byServer: { "203.0.113.10:22": [] }, loaded: true });
    expect(canOnServer("server-1", "applications.view")).toBe(false);
  });

  it("matches a host whatever its case, and defaults the port to 22", () => {
    useServersStore.setState({ servers: [{ ...SERVER, host: "VPS.Example.COM", sshPort: undefined }] });
    useNodePermissionsStore.setState({ byServer: { "vps.example.com:22": ["node.terminal"] }, loaded: true });
    expect(canOnServer("server-1", "node.terminal")).toBe(true);
    expect(canOnServer("server-1", "node.firewall")).toBe(false);
  });

  it("permits when the server id is unknown or missing", () => {
    useNodePermissionsStore.setState({ byServer: { "203.0.113.10:22": [] }, loaded: true });
    expect(canOnServer("nope", "applications.view")).toBe(true);
    expect(canOnServer(null, "applications.view")).toBe(true);
  });
});
