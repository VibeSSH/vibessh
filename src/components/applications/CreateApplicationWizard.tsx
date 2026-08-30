import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { Icon } from "@/components/ui/Icon";
import { IconButton } from "@/components/ui/IconButton";
import { createApplication, detectJavaInstallations } from "@/services/applicationService";
import { listServers, serverSummaryToManagedServer } from "@/services/serverService";
import { useServersStore } from "@/stores/serversStore";
import type { Blueprint, BlueprintField, EnvironmentVariable, JavaInstallation, RuntimeType } from "@/types/application";
import { listBlueprints } from "@/services/applicationService";
import "@/components/servers/AddServerModal.css";
import "@/components/servers/forms.css";
import "./CreateApplicationWizard.css";

interface CreateApplicationWizardProps {
  onClose: () => void;
  onCreated: () => void;
}

const TOTAL_STEPS = 5;

/** Runtime types a Local application can use vs a Remote one - `localProcess` needs no `RuntimeContext.connection`, the other three need one. Intersected with the chosen blueprint's own `supportedRuntimeTypes` to get the real, capability-driven options for a given step (never a hardcoded "always offer Docker" list - see runtime::docker's own scope notes on why neither built-in blueprint even supports it). */
function runtimeTypesForLocation(blueprint: Blueprint, isLocal: boolean): RuntimeType[] {
  return blueprint.supportedRuntimeTypes.filter((rt) => (isLocal ? rt === "localProcess" : rt !== "localProcess"));
}

function fieldValueOrDefault(field: BlueprintField, values: Record<string, unknown>): unknown {
  return field.key in values ? values[field.key] : field.defaultValue;
}

function isFieldFilled(field: BlueprintField, values: Record<string, unknown>): boolean {
  const value = fieldValueOrDefault(field, values);
  if (!field.required) return true;
  if (field.fieldType === "textList") return Array.isArray(value) && value.length > 0;
  if (field.fieldType === "boolean") return typeof value === "boolean";
  return typeof value === "string" ? value.trim().length > 0 : value !== undefined && value !== null;
}

export function CreateApplicationWizard({ onClose, onCreated }: CreateApplicationWizardProps) {
  const { t } = useTranslation();
  const [step, setStep] = useState(1);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const [blueprints, setBlueprints] = useState<Blueprint[]>([]);
  const servers = useServersStore((s) => s.servers);
  const setServers = useServersStore((s) => s.setServers);

  const [serverId, setServerId] = useState<string | null>(null);
  const [name, setName] = useState("");
  const [workingDirectory, setWorkingDirectory] = useState("");
  const [blueprintId, setBlueprintId] = useState<string | null>(null);
  const [runtimeType, setRuntimeType] = useState<RuntimeType | null>(null);
  const [fieldValues, setFieldValues] = useState<Record<string, unknown>>({});
  const [environment, setEnvironment] = useState<EnvironmentVariable[]>([]);

  useEffect(() => {
    listBlueprints()
      .then(setBlueprints)
      .catch(() => setError(t("createApplicationWizard.loadError")));
    if (servers.length === 0) {
      listServers()
        .then((loaded) => setServers(loaded.map(serverSummaryToManagedServer)))
        .catch(() => {
          // No saved servers, or this loaded outside a Tauri webview during
          // development - Local-only is still a fully valid path.
        });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const isLocal = serverId === null;
  const selectedBlueprint = useMemo(() => blueprints.find((b) => b.id === blueprintId) ?? null, [blueprints, blueprintId]);
  const availableRuntimeTypes = useMemo(
    () => (selectedBlueprint ? runtimeTypesForLocation(selectedBlueprint, isLocal) : []),
    [selectedBlueprint, isLocal],
  );

  // Auto-pick the runtime type once it's the only option (always true for
  // Local today, since every built-in blueprint offers exactly one Local
  // runtime) - no point making the user choose from a list of one.
  useEffect(() => {
    if (availableRuntimeTypes.length === 1) {
      setRuntimeType(availableRuntimeTypes[0]);
    } else if (runtimeType && !availableRuntimeTypes.includes(runtimeType)) {
      setRuntimeType(null);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [availableRuntimeTypes]);

  function setFieldValue(key: string, value: unknown) {
    setFieldValues((prev) => ({ ...prev, [key]: value }));
  }

  function updateEnvironmentRow(index: number, patch: Partial<EnvironmentVariable>) {
    setEnvironment((prev) => prev.map((row, i) => (i === index ? { ...row, ...patch } : row)));
  }

  const step1Valid = name.trim().length > 0 && workingDirectory.trim().length > 0;
  const step2Valid = Boolean(blueprintId && runtimeType);
  const step3Valid = selectedBlueprint ? selectedBlueprint.fields.every((field) => isFieldFilled(field, fieldValues)) : false;

  function canAdvanceFrom(currentStep: number): boolean {
    if (currentStep === 1) return step1Valid;
    if (currentStep === 2) return step2Valid;
    if (currentStep === 3) return step3Valid;
    return true;
  }

  async function handleCreate() {
    if (!selectedBlueprint || !runtimeType) return;
    setBusy(true);
    setError(null);
    try {
      await createApplication({
        serverId: serverId ?? undefined,
        name: name.trim(),
        workingDirectory: workingDirectory.trim(),
        blueprintId: selectedBlueprint.id,
        runtimeType,
        environment: environment.filter((row) => row.key.trim().length > 0),
        blueprintInputs: Object.fromEntries(selectedBlueprint.fields.map((field) => [field.key, fieldValueOrDefault(field, fieldValues)])),
      });
      onCreated();
    } catch (err) {
      setError(err instanceof Error ? err.message : t("createApplicationWizard.createError"));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal-panel modal-panel-lg" onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <h2 className="modal-title">{t("createApplicationWizard.title")}</h2>
          <IconButton icon="x" size="sm" onClick={onClose} title={t("common.close")} />
        </div>

        <div className="wizard-steps">
          {Array.from({ length: TOTAL_STEPS }, (_, i) => i + 1).map((dot) => (
            <span
              key={dot}
              className={`wizard-step-dot ${dot === step ? "wizard-step-dot-active" : ""} ${dot < step ? "wizard-step-dot-done" : ""}`}
            />
          ))}
        </div>
        <p className="wizard-step-title">{t(`createApplicationWizard.step${step}Title`)}</p>

        <div className="modal-body">
          {error && <p className="form-note form-note-danger form-note-spaced">{error}</p>}

          <div className="wizard-step-body">
            {step === 1 && (
              <>
                <label className="form-field">
                  <span className="form-label">{t("createApplicationWizard.location")}</span>
                  <div className="wizard-location-options">
                    <button type="button" className={`wizard-location-option ${isLocal ? "wizard-location-option-active" : ""}`} onClick={() => setServerId(null)}>
                      <Icon name="layout-grid" size={16} />
                      {t("createApplicationWizard.locationLocal")}
                    </button>
                    {servers.map((server) => (
                      <button
                        key={server.id}
                        type="button"
                        className={`wizard-location-option ${serverId === server.id ? "wizard-location-option-active" : ""}`}
                        onClick={() => setServerId(server.id)}
                      >
                        <Icon name="server" size={16} />
                        {server.name}
                      </button>
                    ))}
                  </div>
                  {servers.length === 0 && <p className="form-note">{t("createApplicationWizard.noServersNote")}</p>}
                </label>

                <label className="form-field">
                  <span className="form-label">{t("createApplicationWizard.name")}</span>
                  <input className="form-input" value={name} onChange={(e) => setName(e.target.value)} placeholder={t("createApplicationWizard.namePlaceholder")} />
                </label>

                <label className="form-field">
                  <span className="form-label">{t("createApplicationWizard.workingDirectory")}</span>
                  <input
                    className="form-input"
                    value={workingDirectory}
                    onChange={(e) => setWorkingDirectory(e.target.value)}
                    placeholder={isLocal ? t("createApplicationWizard.workingDirectoryPlaceholderLocal") : t("createApplicationWizard.workingDirectoryPlaceholderRemote")}
                  />
                  <p className="form-note">{t("createApplicationWizard.workingDirectoryNote")}</p>
                </label>
              </>
            )}

            {step === 2 && (
              <>
                <label className="form-field">
                  <span className="form-label">{t("createApplicationWizard.blueprint")}</span>
                  <div className="wizard-blueprint-options">
                    {blueprints.map((blueprint) => (
                      <button
                        key={blueprint.id}
                        type="button"
                        className={`wizard-blueprint-option ${blueprintId === blueprint.id ? "wizard-blueprint-option-active" : ""}`}
                        onClick={() => setBlueprintId(blueprint.id)}
                      >
                        <span className="wizard-blueprint-option-name">{blueprint.name}</span>
                        <span className="wizard-blueprint-option-description">{blueprint.description}</span>
                      </button>
                    ))}
                  </div>
                </label>

                {selectedBlueprint && availableRuntimeTypes.length > 1 && (
                  <label className="form-field">
                    <span className="form-label">{t("createApplicationWizard.runtimeType")}</span>
                    <select className="form-input" value={runtimeType ?? ""} onChange={(e) => setRuntimeType(e.target.value as RuntimeType)}>
                      <option value="" disabled>
                        {t("createApplicationWizard.runtimeTypePlaceholder")}
                      </option>
                      {availableRuntimeTypes.map((rt) => (
                        <option key={rt} value={rt}>
                          {t(`createApplicationWizard.runtimeTypeOption.${rt}`)}
                        </option>
                      ))}
                    </select>
                  </label>
                )}
                {selectedBlueprint && availableRuntimeTypes.length === 0 && (
                  <p className="form-note form-note-danger">{t("createApplicationWizard.noRuntimeForLocation")}</p>
                )}
              </>
            )}

            {step === 3 && selectedBlueprint && (
              <div className="wizard-field-list">
                {selectedBlueprint.fields.map((field) => (
                  <BlueprintFieldInput
                    key={field.key}
                    field={field}
                    value={fieldValueOrDefault(field, fieldValues)}
                    onChange={(v) => setFieldValue(field.key, v)}
                    serverId={serverId}
                  />
                ))}
              </div>
            )}

            {step === 4 && (
              <div className="wizard-field-list">
                <p className="form-note">{t("createApplicationWizard.environmentNote")}</p>
                {environment.map((row, index) => (
                  <div key={index} className="wizard-env-row">
                    <input
                      className="form-input"
                      placeholder={t("createApplicationWizard.environmentKeyPlaceholder")}
                      value={row.key}
                      onChange={(e) => updateEnvironmentRow(index, { key: e.target.value })}
                    />
                    <input
                      className="form-input"
                      placeholder={t("createApplicationWizard.environmentValuePlaceholder")}
                      value={row.value}
                      onChange={(e) => updateEnvironmentRow(index, { value: e.target.value })}
                    />
                    <IconButton
                      icon="trash"
                      size="sm"
                      danger
                      title={t("createApplicationWizard.removeVariableAria")}
                      onClick={() => setEnvironment((prev) => prev.filter((_, i) => i !== index))}
                    />
                  </div>
                ))}
                <Button variant="secondary" size="sm" onClick={() => setEnvironment((prev) => [...prev, { key: "", value: "" }])}>
                  <Icon name="plus" size={14} />
                  {t("createApplicationWizard.addVariable")}
                </Button>
              </div>
            )}

            {step === 5 && selectedBlueprint && runtimeType && (
              <div className="wizard-review-grid">
                <span className="wizard-review-label">{t("createApplicationWizard.name")}</span>
                <span className="wizard-review-value">{name}</span>
                <span className="wizard-review-label">{t("createApplicationWizard.location")}</span>
                <span className="wizard-review-value">
                  {isLocal ? t("createApplicationWizard.locationLocal") : servers.find((s) => s.id === serverId)?.name ?? serverId}
                </span>
                <span className="wizard-review-label">{t("createApplicationWizard.blueprint")}</span>
                <span className="wizard-review-value">{selectedBlueprint.name}</span>
                <span className="wizard-review-label">{t("createApplicationWizard.runtimeType")}</span>
                <span className="wizard-review-value">{t(`createApplicationWizard.runtimeTypeOption.${runtimeType}`)}</span>
                <span className="wizard-review-label">{t("createApplicationWizard.workingDirectory")}</span>
                <span className="wizard-review-value">{workingDirectory}</span>
                <span className="wizard-review-label">{t("createApplicationWizard.environmentReviewLabel")}</span>
                <span className="wizard-review-value">
                  {environment.filter((row) => row.key.trim()).length || t("createApplicationWizard.environmentReviewNone")}
                </span>
              </div>
            )}
          </div>
        </div>

        <div className="form-actions form-actions-split wizard-footer">
          <Button variant="secondary" onClick={() => (step === 1 ? onClose() : setStep((s) => s - 1))} disabled={busy}>
            {step === 1 ? t("common.cancel") : t("common.back")}
          </Button>
          {step < TOTAL_STEPS ? (
            <Button onClick={() => setStep((s) => s + 1)} disabled={!canAdvanceFrom(step)}>
              {t("createApplicationWizard.next")}
            </Button>
          ) : (
            <Button onClick={handleCreate} disabled={busy}>
              {busy ? t("common.saving") : t("common.create")}
            </Button>
          )}
        </div>
      </div>
    </div>
  );
}

interface BlueprintFieldInputProps {
  field: BlueprintField;
  value: unknown;
  onChange: (value: unknown) => void;
  serverId: string | null;
}

function BlueprintFieldInput({ field, value, onChange, serverId }: BlueprintFieldInputProps) {
  if (field.fieldType === "javaVersion") {
    return <JavaVersionFieldInput field={field} value={value} onChange={onChange} serverId={serverId} />;
  }

  if (field.fieldType === "boolean") {
    return (
      <label className="form-field">
        <span className="form-label">
          {field.label}
          {field.required ? " *" : ""}
        </span>
        <input type="checkbox" checked={Boolean(value)} onChange={(e) => onChange(e.target.checked)} />
        {field.helpText && <p className="form-note">{field.helpText}</p>}
      </label>
    );
  }

  if (field.fieldType === "textList") {
    const text = Array.isArray(value) ? value.join("\n") : "";
    return (
      <label className="form-field">
        <span className="form-label">
          {field.label}
          {field.required ? " *" : ""}
        </span>
        <textarea
          className="form-input form-textarea"
          value={text}
          onChange={(e) =>
            onChange(
              e.target.value
                .split("\n")
                .map((line) => line.trim())
                .filter((line) => line.length > 0),
            )
          }
        />
        {field.helpText && <p className="form-note">{field.helpText}</p>}
      </label>
    );
  }

  return (
    <label className="form-field">
      <span className="form-label">
        {field.label}
        {field.required ? " *" : ""}
      </span>
      <input
        className="form-input"
        type={field.fieldType === "number" ? "number" : "text"}
        value={typeof value === "string" || typeof value === "number" ? value : ""}
        onChange={(e) => onChange(field.fieldType === "number" ? Number(e.target.value) : e.target.value)}
      />
      {field.helpText && <p className="form-note">{field.helpText}</p>}
    </label>
  );
}

interface JavaVersionFieldInputProps {
  field: BlueprintField;
  value: unknown;
  onChange: (value: unknown) => void;
  serverId: string | null;
}

/** A picker built from real, detected Java installations (local scan, or a remote SSH scan when a server is chosen) - falls back to a plain path input when nothing was detected, or when the user explicitly asks for a custom path via the dropdown's own option for it. */
function JavaVersionFieldInput({ field, value, onChange, serverId }: JavaVersionFieldInputProps) {
  const { t } = useTranslation();
  const [installations, setInstallations] = useState<JavaInstallation[] | null>(null);
  const [detecting, setDetecting] = useState(true);
  const [detectError, setDetectError] = useState<string | null>(null);
  const [customPath, setCustomPath] = useState(false);

  useEffect(() => {
    setDetecting(true);
    setInstallations(null);
    setDetectError(null);
    detectJavaInstallations(serverId ?? undefined)
      .then(setInstallations)
      .catch((err) => {
        setInstallations([]);
        setDetectError(err instanceof Error ? err.message : String(err));
      })
      .finally(() => setDetecting(false));
  }, [serverId]);

  const currentValue = typeof value === "string" ? value : "";
  const label = (
    <span className="form-label">
      {field.label}
      {field.required ? " *" : ""}
    </span>
  );

  if (detecting) {
    return (
      <label className="form-field">
        {label}
        <p className="form-note">{t("createApplicationWizard.detectingJava")}</p>
      </label>
    );
  }

  const hasDetected = installations !== null && installations.length > 0;
  if (!hasDetected || customPath) {
    return (
      <label className="form-field">
        {label}
        <input className="form-input" value={currentValue} onChange={(e) => onChange(e.target.value)} />
        {hasDetected && (
          <Button variant="ghost" size="sm" onClick={() => setCustomPath(false)}>
            {t("createApplicationWizard.backToDetectedJava")}
          </Button>
        )}
        {!hasDetected && <p className="form-note">{t("createApplicationWizard.noJavaDetected")}</p>}
        {detectError && <p className="form-note form-note-danger">{t("createApplicationWizard.javaDetectError", { error: detectError })}</p>}
      </label>
    );
  }

  const matchesDetected = installations.some((installation) => installation.path === currentValue);
  return (
    <label className="form-field">
      {label}
      <select
        className="form-input"
        value={matchesDetected ? currentValue : ""}
        onChange={(e) => (e.target.value === "__custom__" ? setCustomPath(true) : onChange(e.target.value))}
      >
        <option value="" disabled>
          {t("createApplicationWizard.chooseJava")}
        </option>
        {installations.map((installation) => (
          <option key={installation.path} value={installation.path}>
            {installation.label} — {installation.path}
          </option>
        ))}
        <option value="__custom__">{t("createApplicationWizard.customJavaPath")}</option>
      </select>
      {field.helpText && <p className="form-note">{field.helpText}</p>}
    </label>
  );
}
