import type { RuntimeType } from "./application";

/** Mirrors the Rust `TemplateEnvironmentVariable` DTO. */
export interface TemplateEnvironmentVariable {
  key: string;
  /**
   * Always empty when `isSecret` - the value is dropped before the template
   * reaches disk, so a saved secret travels as its name and the wizard asks
   * for it again. See the Rust model for why that is the feature rather than
   * a gap.
   */
  value: string;
  isSecret: boolean;
}

/** Mirrors the Rust `ApplicationTemplate` DTO. */
export interface ApplicationTemplate {
  id: string;
  name: string;
  blueprintId: string;
  runtimeType: RuntimeType;
  /** Keyed by `BlueprintField.key`, the same shape the wizard collects. */
  fieldValues: Record<string, unknown>;
  environment: TemplateEnvironmentVariable[];
  createdAt: string;
}
