/** Mirrors the Rust `RuntimeType` enum. */
export type RuntimeType = "localProcess" | "remoteProcess" | "systemd" | "docker";

/** Mirrors the Rust `ApplicationStatus` enum - last-known, always refreshed from the runtime, never trusted as sole truth. */
export type ApplicationStatus = "unknown" | "starting" | "running" | "stopping" | "stopped" | "failed";

/** `value` is always empty for a secret row (`isSecret === true`) on every
 * plain read - the backend never sends a secret's real value back over IPC,
 * only that it is one (see the Rust `EnvironmentVariable::value` doc
 * comment). Submitting a secret row with an empty `value` on save means
 * "keep the current value," not "clear it." */
export interface EnvironmentVariable {
  key: string;
  value: string;
  isSecret: boolean;
}

export type PortProtocol = "tcp" | "udp";

/** Mirrors the Rust `PortVisibility` enum (Etap M4's "Application Network") - the user-facing intent behind a port. `bindAddress` is still what `runtime::docker` actually publishes on; the service layer computes it from this on every save, so the UI only ever needs to show/collect *this*, never a raw bind address, except for `"custom"`. */
export type PortVisibility = "public" | "vibeNetwork" | "localhost" | "custom";

export interface ApplicationPort {
  id: string;
  applicationId: string;
  name: string;
  protocol: PortProtocol;
  bindAddress: string;
  internalPort: number;
  externalPort?: number;
  visibility: PortVisibility;
  required: boolean;
  createdAt: string;
  updatedAt: string;
}

/** What add/update submit - `required` always omitted from the UI (defaults to false server-side): a port declared through this tab is always user-removable, "required" is a blueprint-authored concept this UI doesn't expose a way to set. `bindAddress` only actually matters when `visibility` is `"custom"` - still required on the wire (the service layer overwrites it otherwise), so it's always sent as at least an empty string. */
export interface PortInput {
  name: string;
  protocol: PortProtocol;
  bindAddress: string;
  internalPort: number;
  externalPort?: number;
  visibility: PortVisibility;
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

/** What `setApplicationResourceLimits` submits - mirrors the Rust `SetResourceLimitsInput` DTO. `undefined` clears that particular limit rather than leaving it untouched. `cpuLimitCores` is accepted for `"docker"`/`"systemd"`/`"remoteProcess"`; `memoryLimitMb` only for `"docker"`/`"systemd"` - a bare SSH-launched process has no cgroup to cap memory through, see the Rust `set_application_resource_limits`'s own doc comment. `"localProcess"` rejects both. */
export interface SetResourceLimitsInput {
  memoryLimitMb?: number;
  cpuLimitCores?: number;
}

/** The subset of a Docker/systemd application's own `runtimeConfig` shape this UI reads/writes - the rest of that shape (image, command, ...) is opaque here, same as `ApplicationDetail.runtimeConfig`'s own `unknown` type says. */
export interface ResourceLimitsConfig {
  memoryLimitMb?: number;
  cpuLimitCores?: number;
}

/** The one field of a Docker application's own `runtimeConfig` the Docker Image card reads/writes - see `ResourceLimitsConfig`'s own doc comment for why the rest of that shape stays opaque here. */
export interface DockerImageConfig {
  image?: string;
}

/** One registry's login - reused by every Application that pulls from that host. `password` never comes back from the backend, only ever sent when setting/replacing it. */
export interface RegistryCredential {
  id: string;
  registry: string;
  username: string;
  createdAt: string;
}

export interface SetRegistryCredentialInput {
  registry: string;
  username: string;
  password: string;
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
  /** Ids of the other applications this one is allowed to reach on its node.
   * Symmetric - if A lists B, B lists A. See `listApplicationLinks`. */
  links: string[];
}

export interface ResourceUsage {
  cpuPercent?: number;
  ramBytes?: number;
  uptimeSeconds?: number;
}

export type BackupKind = "manual" | "scheduled";

/** Mirrors the Rust `ApplicationBackup` DTO - a `.zip` of the working directory, listed via `listApplicationBackups`. */
export interface ApplicationBackup {
  id: string;
  applicationId: string;
  fileName: string;
  sizeBytes: number;
  kind: BackupKind;
  /** Present when this backup was also uploaded to the configured S3-compatible destination - see the Rust `ApplicationBackup::s3_key` doc comment. */
  s3Key?: string;
  createdAt: string;
}

/** Mirrors the Rust `BackupSchedule` DTO - absent config reads back as `{enabled: false, intervalHours: 24, retentionCount: 5}`, see `getApplicationBackupSchedule`'s own doc comment. `retentionMaxAgeDays`/`retentionMaxTotalBytes` are `undefined` when that rule is off. */
export interface BackupSchedule {
  enabled: boolean;
  intervalHours: number;
  retentionCount: number;
  retentionMaxAgeDays?: number;
  retentionMaxTotalBytes?: number;
}

/** Mirrors the Rust `BackupDestinationConfig` DTO - one global S3-compatible destination every Application's backups can additionally upload to. Never carries the secret access key - see the Rust struct's own doc comment. */
export interface BackupDestinationConfig {
  enabled: boolean;
  endpoint: string;
  region: string;
  bucket: string;
  accessKeyId: string;
  pathPrefix: string;
  pathStyle: boolean;
}

/** What `setBackupDestination` submits - mirrors the Rust `SetBackupDestinationInput` DTO. `secretAccessKey` blank means "keep the currently stored secret." */
export interface SetBackupDestinationInput {
  enabled: boolean;
  endpoint: string;
  region: string;
  bucket: string;
  accessKeyId: string;
  pathPrefix: string;
  pathStyle: boolean;
  secretAccessKey: string;
}

export type BlueprintFeature = "console" | "logs" | "environment" | "ports" | "healthCheck" | "databases" | "files";

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

/** A "Quick Files" shortcut on the Files tab - a path this blueprint knows is worth surfacing directly (Paper's server.properties, Velocity's velocity.toml, ...). Opens through the exact same Files/editor UI as browsing to it by hand. */
export interface KnownFile {
  path: string;
  label: string;
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
  knownFiles: KnownFile[];
  isBuiltin: boolean;
}
