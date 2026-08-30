/** Mirrors the Rust `RuntimeType` enum. */
export type RuntimeType = "localProcess" | "remoteProcess" | "systemd" | "docker";

/** Mirrors the Rust `ApplicationStatus` enum - last-known, always refreshed from the runtime, never trusted as sole truth. */
export type ApplicationStatus = "unknown" | "starting" | "running" | "stopping" | "stopped" | "failed";

export interface EnvironmentVariable {
  key: string;
  value: string;
}

export type PortProtocol = "tcp" | "udp";

export interface ApplicationPort {
  id: string;
  applicationId: string;
  name: string;
  protocol: PortProtocol;
  bindAddress: string;
  internalPort: number;
  externalPort?: number;
  required: boolean;
  createdAt: string;
  updatedAt: string;
}

/** What add/update submit - `required` always omitted from the UI (defaults to false server-side): a port declared through this tab is always user-removable, "required" is a blueprint-authored concept this UI doesn't expose a way to set. */
export interface PortInput {
  name: string;
  protocol: PortProtocol;
  bindAddress: string;
  internalPort: number;
  externalPort?: number;
}

/** Mirrors the Rust `HealthCheckType` enum - what `getApplicationHealth` probes beyond "is the process still running" (that check always happens first, regardless of this setting). */
export type HealthCheckType = "process" | "tcp" | "http" | "minecraftStatus";

/** Mirrors the Rust `HealthStatus` enum's adjacently-tagged `Serialize` shape (`{"status":"healthy"}` / `{"status":"unhealthy","reason":"..."}` / `{"status":"unknown"}`). */
export type HealthStatus = { status: "healthy" } | { status: "unhealthy"; reason: string } | { status: "unknown" };

/** What `setApplicationHealthCheck` submits - mirrors the Rust `SetHealthCheckInput` DTO. `portId` is required unless `healthCheckType` is `"process"`; `httpPath` is required (and must start with `/`) only for `"http"`. */
export interface SetHealthCheckInput {
  healthCheckType: HealthCheckType;
  portId?: string;
  httpPath?: string;
}

/** What `setApplicationResourceLimits` submits - mirrors the Rust `SetResourceLimitsInput` DTO. `undefined` clears that particular limit rather than leaving it untouched. Only accepted for a `"docker"`/`"systemd"` application - see that function's own doc comment for why `"localProcess"`/`"remoteProcess"` reject it outright. */
export interface SetResourceLimitsInput {
  memoryLimitMb?: number;
  cpuLimitCores?: number;
}

/** The subset of a Docker/systemd application's own `runtimeConfig` shape this UI reads/writes - the rest of that shape (image, command, ...) is opaque here, same as `ApplicationDetail.runtimeConfig`'s own `unknown` type says. */
export interface ResourceLimitsConfig {
  memoryLimitMb?: number;
  cpuLimitCores?: number;
}

export interface Application {
  id: string;
  /** Undefined = Local. There is no separate "location" field - same single source of truth as the Rust `Application::location()` derivation. */
  serverId?: string;
  name: string;
  description?: string;
  blueprintId: string;
  blueprintVersion: number;
  runtimeType: RuntimeType;
  workingDirectory: string;
  status: ApplicationStatus;
  lastStatusCheckAt?: string;
  healthCheckType: HealthCheckType;
  /** References an `ApplicationPort` - `undefined` for `"process"`, and also `undefined` if the port a check pointed at has since been removed. */
  healthCheckPortId?: string;
  healthCheckHttpPath?: string;
  createdAt: string;
  updatedAt: string;
}

/** `#[serde(flatten)]` on the Rust side - environment/ports/runtimeConfig/metadata sit alongside Application's own fields in the same object, not nested under an `application` key. */
export interface ApplicationDetail extends Application {
  environment: EnvironmentVariable[];
  ports: ApplicationPort[];
  runtimeConfig: unknown;
  metadata: unknown;
}

export interface ResourceUsage {
  cpuPercent?: number;
  ramBytes?: number;
  uptimeSeconds?: number;
}

export type BlueprintFeature = "console" | "logs" | "environment" | "ports" | "healthCheck" | "databases";

export type BlueprintFieldType = "text" | "path" | "number" | "boolean" | "textList" | "javaVersion" | "papermcVersion";

/** A real, detected Java installation - see `detectJavaInstallations`. One entry per major version (e.g. only one "21" even if several vendors are installed) - `majorVersion` is what the picker shows ("Java 21"), `label`/`path` are the full detail behind it. */
export interface JavaInstallation {
  path: string;
  label: string;
  majorVersion: string;
}

export interface BlueprintField {
  key: string;
  label: string;
  fieldType: BlueprintFieldType;
  required: boolean;
  defaultValue?: unknown;
  helpText?: string;
}

export interface Blueprint {
  id: string;
  name: string;
  description: string;
  schemaVersion: number;
  blueprintVersion: number;
  supportedRuntimeTypes: RuntimeType[];
  features: BlueprintFeature[];
  fields: BlueprintField[];
  isBuiltin: boolean;
}
