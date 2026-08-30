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

export type BlueprintFeature = "console" | "logs" | "environment" | "ports";

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
