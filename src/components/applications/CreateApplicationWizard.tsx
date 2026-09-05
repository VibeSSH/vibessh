import { Fragment, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { open } from "@tauri-apps/plugin-shell";
import { Button } from "@/components/ui/Button";
import { Checkbox } from "@/components/ui/Checkbox";
import { HelpHint } from "@/components/ui/HelpHint";
import { Icon } from "@/components/ui/Icon";
import { Select } from "@/components/ui/Select";
import { IconButton } from "@/components/ui/IconButton";
import { useModalDialog } from "@/hooks/useModalDialog";
import { createApplication, detectJavaInstallations, listPaperVersions, listPurpurVersions, listVelocityVersions, listWaterfallVersions } from "@/services/applicationService";
import { installDocker, listServers, probeServerCapabilities, serverSummaryToManagedServer } from "@/services/serverService";
import { useServersStore } from "@/stores/serversStore";
import { toastSuccess } from "@/stores/toastStore";
import { translateBlueprint } from "@/i18n/blueprintTranslations";
import type { Blueprint, BlueprintField, EnvironmentVariable, JavaInstallation, RuntimeType } from "@/types/application";
import { listBlueprints } from "@/services/applicationService";
import "@/components/servers/AddServerModal.css";
import "@/components/servers/forms.css";
import "./CreateApplicationWizard.css";
import { errorMessage } from "@/services/tauri";
import { BlueprintIcon } from "@/components/applications/BlueprintIcon";

interface CreateApplicationWizardProps {
  onClose: () => void;
  onCreated: () => void;
}

const TOTAL_STEPS = 5;

/** Runtime types a Local application can use vs a Remote one - `localProcess` needs no `RuntimeContext.connection`, the other three need one. Intersected with the chosen blueprint's own `supportedRuntimeTypes` to get the real, capability-driven options for a given step (never a hardcoded "always offer Docker" list - only `generic-docker` declares Docker support, so it's the only blueprint that offers it). */
function runtimeTypesForLocation(blueprint: Blueprint, isLocal: boolean): RuntimeType[] {
  // Docker is offered locally as well as remotely. It used to be remote-only,
  // not by design but because the Docker runtime spoke POSIX shell over SSH -
  // every invocation a `sudo docker ...` string. It now builds arguments and
  // hands them to whichever daemon the Application belongs to, and an
  // Application with no Node is exactly the one that means this machine.
  //
  // `localProcess` stays the other way round: it runs a program here, so it
  // has nothing to say about a remote Node.
  return blueprint.supportedRuntimeTypes.filter((rt) => (isLocal ? rt === "localProcess" || rt === "docker" : rt !== "localProcess"));
}

export function fieldValueOrDefault(field: BlueprintField, values: Record<string, unknown>): unknown {
  return field.key in values ? values[field.key] : field.defaultValue;
}

/** Splits on any run of whitespace, not just newlines - one flag per line and a single space-separated line both work the same way, matching how these values are actually space-delimited on a real command line. */
function splitTextListInput(text: string): string[] {
  return text.trim().split(/\s+/).filter((token) => token.length > 0);
}

/** A filesystem-safe directory name from whatever the user has typed as the application's name so far - lowercased, non-alphanumerics collapsed to a single hyphen, no leading/trailing hyphen. Falls back to "app" for an empty/all-punctuation name so the suggested path is never left with a trailing slash and nothing after it. */
function slugify(value: string): string {
  const slug = value
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return slug || "app";
}

/** Not real language content (JVM flag syntax is the same in every locale) - a plain constant rather than an i18n key, same as the app's other command-line examples (e.g. "-Xmx2G") already sit inline in translated help text rather than being translated themselves. */
const JVM_ARGS_PLACEHOLDER = "-Xms1G\n-Xmx2G";

/** Matches a bare "java"/"java.exe", or a path ending in one - the binary itself, which has its own dedicated "Java version" field and is never a real JVM flag or program argument. */
const JAVA_BINARY_TOKEN = /(^|[\\/])java(\.exe)?$/i;

/** A generator like flags.sh hands out one copy-pasteable line - `java -Xmx2G ... -jar server.jar nogui` - meant to be run directly in a shell, not split across VibeSSH's separate Java version / JVM arguments / jar file / program arguments fields. Detects that shape (a `-jar` token present) and splits it back into those pieces - `null` if the text doesn't look like this at all, e.g. plain flags with no `-jar` in them. */
function parseFullJavaCommand(tokens: string[]): { jvmArgs: string[]; jarPath: string; programArgs: string[] } | null {
  const jarIndex = tokens.indexOf("-jar");
  if (jarIndex === -1 || jarIndex + 1 >= tokens.length) return null;

  const jvmArgs = tokens.slice(0, jarIndex).filter((token) => !JAVA_BINARY_TOKEN.test(token));

  return { jvmArgs, jarPath: tokens[jarIndex + 1], programArgs: tokens.slice(jarIndex + 2) };
}

function isFieldFilled(field: BlueprintField, values: Record<string, unknown>): boolean {
  const value = fieldValueOrDefault(field, values);
  if (!field.required) return true;
  if (field.fieldType === "textList") return Array.isArray(value) && value.length > 0;
  if (field.fieldType === "boolean") return typeof value === "boolean";
  return typeof value === "string" ? value.trim().length > 0 : value !== undefined && value !== null;
}

/** The step-3 fields (Egg-specific: Java/Minecraft version, JVM flags, the EULA checkbox, ...) as a read-only summary line for the final review step - so "what am I about to create" is actually answerable there instead of only listing the fixed name/location/runtime fields every blueprint shares. Joins a textList with spaces (JVM/program arguments read as the command line they actually become), not commas or newlines. */
export function formatFieldValueForReview(field: BlueprintField, value: unknown, yesLabel: string, noLabel: string): string {
  if (field.fieldType === "boolean") return value ? yesLabel : noLabel;
  if (field.fieldType === "textList") {
    const items = Array.isArray(value) ? value : [];
    return items.length > 0 ? items.join(" ") : "—";
  }
  if (typeof value === "string") return value.trim().length > 0 ? value : "—";
  if (typeof value === "number") return String(value);
  return "—";
}

export function CreateApplicationWizard({ onClose, onCreated }: CreateApplicationWizardProps) {
  const { t, i18n } = useTranslation();
  const [step, setStep] = useState(1);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [installingDocker, setInstallingDocker] = useState(false);
  const [dockerInstallError, setDockerInstallError] = useState<string | null>(null);

  const [blueprints, setBlueprints] = useState<Blueprint[]>([]);
  const servers = useServersStore((s) => s.servers);
  const setServers = useServersStore((s) => s.setServers);
  const upsertServer = useServersStore((s) => s.upsertServer);

  const [serverId, setServerId] = useState<string | null>(null);
  const [name, setName] = useState("");
  const [workingDirectory, setWorkingDirectory] = useState("");
  const [workingDirectoryTouched, setWorkingDirectoryTouched] = useState(false);
  const [blueprintId, setBlueprintId] = useState<string | null>(null);
  const [runtimeType, setRuntimeType] = useState<RuntimeType | null>(null);
  const [fieldValues, setFieldValues] = useState<Record<string, unknown>>({});
  const [environment, setEnvironment] = useState<EnvironmentVariable[]>([]);
  const backdrop = useModalDialog(onClose, { labelledBy: "createapplicationwizard-dialog-title-1" });

  useEffect(() => {
    listBlueprints()
      .then((loaded) => setBlueprints(loaded.map((b) => translateBlueprint(b, i18n.language))))
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

  // A Pterodactyl-style suggested path, not a requirement - a remote
  // application always runs inside its own bind-mounted Docker directory
  // (or a plain SSH working directory for non-Docker runtimes) either way,
  // so a sensible default beats an empty required field. Stops suggesting
  // the moment the user edits the field themselves; Local has no such
  // convention to suggest, so it's left for the placeholder text alone.
  useEffect(() => {
    if (workingDirectoryTouched) return;
    setWorkingDirectory(isLocal ? "" : `/home/container/${slugify(name)}`);
  }, [name, isLocal, workingDirectoryTouched]);

  const selectedBlueprint = useMemo(() => blueprints.find((b) => b.id === blueprintId) ?? null, [blueprints, blueprintId]);
  const availableRuntimeTypes = useMemo(
    () => (selectedBlueprint ? runtimeTypesForLocation(selectedBlueprint, isLocal) : []),
    [selectedBlueprint, isLocal],
  );
  const selectedServer = useMemo(() => servers.find((s) => s.id === serverId) ?? null, [servers, serverId]);

  // Docker capability is only ever known for an SSH-mode server after a
  // real probe has run (Etap M1) - agent-mode servers instead carry a live
  // `capabilities` reading from their own handshake, which needs no probe.
  // Fires once per selected SSH-mode server whose capabilities aren't known
  // yet - not on every render, and never for Local (nothing to probe).
  useEffect(() => {
    if (!selectedServer || selectedServer.connectionMode !== "ssh" || selectedServer.nodeCapabilities) return;
    let cancelled = false;
    probeServerCapabilities(selectedServer.id)
      .then((nodeCapabilities) => {
        if (!cancelled) upsertServer({ ...selectedServer, nodeCapabilities });
      })
      .catch(() => {
        // Best-effort - an unreachable server just stays "unknown" here,
        // same as it already is everywhere else in the app; the wizard
        // still lets the user try (DockerRuntime::validate gives the real,
        // authoritative answer at creation time either way).
      });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedServer?.id, selectedServer?.connectionMode, selectedServer?.nodeCapabilities]);

  // `false` (not `undefined`) is a real, known-negative answer - a probe or
  // a live handshake actually said "no Docker here." `undefined` (still
  // probing, or an SSH server never successfully probed) deliberately
  // shows nothing, since that's not something to warn about yet.
  const dockerCapabilityWarning =
    runtimeType === "docker" &&
    selectedServer &&
    (selectedServer.connectionMode === "agent" ? selectedServer.capabilities?.docker === false : selectedServer.nodeCapabilities?.docker === false);
  // Agent-mode Nodes have no SSH session this could run over - only an
  // SSH-mode server can offer the one-click install below; an Agent-mode
  // one just keeps the plain warning telling the user to install it
  // themselves.
  const canInstallDocker = Boolean(dockerCapabilityWarning) && selectedServer?.connectionMode === "ssh";

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

  async function handleInstallDocker() {
    if (!selectedServer) return;
    setInstallingDocker(true);
    setDockerInstallError(null);
    try {
      const nodeCapabilities = await installDocker(selectedServer.id);
      upsertServer({ ...selectedServer, nodeCapabilities });
      if (nodeCapabilities.docker) {
        toastSuccess(t("createApplicationWizard.dockerInstalled", { name: selectedServer.name }));
      } else {
        setDockerInstallError(t("createApplicationWizard.dockerInstallError"));
      }
    } catch (err) {
      setDockerInstallError(errorMessage(err, t));
    } finally {
      setInstallingDocker(false);
    }
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
      setError(errorMessage(err, t));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="modal-backdrop" {...backdrop.backdropProps}>
      <div className="modal-panel modal-panel-lg" {...backdrop.panelProps}>
        <div className="modal-header">
          <h2 className="modal-title" id="createapplicationwizard-dialog-title-1">{t("createApplicationWizard.title")}</h2>
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
                    onChange={(e) => {
                      setWorkingDirectory(e.target.value);
                      setWorkingDirectoryTouched(true);
                    }}
                    placeholder={isLocal ? t("createApplicationWizard.workingDirectoryPlaceholderLocal") : t("createApplicationWizard.workingDirectoryPlaceholderRemote")}
                  />
                  <p className="form-note">
                    {isLocal ? t("createApplicationWizard.workingDirectoryNote") : t("createApplicationWizard.workingDirectoryNoteRemote")}
                  </p>
                </label>
              </>
            )}

            {step === 2 && (
              <>
                <label className="form-field">
                  <span className="form-label">
                    {t("createApplicationWizard.blueprint")}
                    <HelpHint label={t("createApplicationWizard.eggHint")} />
                  </span>
                  {/* A grid of icon + name, not a column of paragraphs. Twelve
                      full descriptions stacked vertically made this step taller
                      than the screen and turned choosing between two known
                      options into a scrolling exercise. The description is not
                      lost - it moves below, for whichever one is selected, and
                      onto the tile's tooltip for the rest. */}
                  <div className="wizard-blueprint-grid">
                    {blueprints.map((blueprint) => (
                      <button
                        key={blueprint.id}
                        type="button"
                        title={blueprint.description}
                        aria-pressed={blueprintId === blueprint.id}
                        className={`wizard-blueprint-tile ${blueprintId === blueprint.id ? "wizard-blueprint-tile-active" : ""}`}
                        onClick={() => setBlueprintId(blueprint.id)}
                      >
                        <span className="wizard-blueprint-tile-icon">
                          <BlueprintIcon blueprintId={blueprint.id} size={20} />
                        </span>
                        <span className="wizard-blueprint-tile-name">{blueprint.name}</span>
                      </button>
                    ))}
                  </div>
                  {selectedBlueprint && <p className="wizard-blueprint-description">{selectedBlueprint.description}</p>}
                </label>

                {selectedBlueprint && availableRuntimeTypes.length > 1 && (
                  <label className="form-field">
                    <span className="form-label">{t("createApplicationWizard.runtimeType")}</span>
                    <Select
                      value={runtimeType ?? ""}
                      onChange={(value) => setRuntimeType(value as RuntimeType)}
                      placeholder={t("createApplicationWizard.runtimeTypePlaceholder")}
                      items={availableRuntimeTypes.map((rt) => ({
                        value: rt,
                        label: t(`createApplicationWizard.runtimeTypeOption.${rt}`),
                      }))}
                    />
                  </label>
                )}
                {selectedBlueprint && availableRuntimeTypes.length === 0 && (
                  <p className="form-note form-note-danger">{t("createApplicationWizard.noRuntimeForLocation")}</p>
                )}
                {dockerCapabilityWarning && (
                  <div className="wizard-docker-warning">
                    <p className="form-note form-note-danger">{t("createApplicationWizard.dockerNotDetected")}</p>
                    {canInstallDocker && (
                      <Button type="button" variant="secondary" size="sm" onClick={handleInstallDocker} disabled={installingDocker}>
                        <Icon name="download" size={14} />
                        {installingDocker ? t("createApplicationWizard.installingDocker") : t("createApplicationWizard.installDockerButton")}
                      </Button>
                    )}
                    {dockerInstallError && <p className="form-note form-note-danger">{dockerInstallError}</p>}
                  </div>
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
                    onChange={(v) => {
                      // A pasted full "java -Xmx2G ... -jar server.jar nogui"
                      // line (the exact shape a flags generator hands out)
                      // belongs across separate fields, not crammed into
                      // this one as one broken argument - see
                      // parseFullJavaCommand's own doc comment. Applies to
                      // any blueprint with a "jvmArgs" field (generic-java,
                      // paper) - paper has no "jarPath" field of its own
                      // (the jar is auto-downloaded), so that piece is just
                      // dropped there rather than set somewhere nonexistent.
                      if (field.key === "jvmArgs" && Array.isArray(v)) {
                        const parsed = parseFullJavaCommand(v);
                        if (parsed) {
                          const hasJarPathField = selectedBlueprint.fields.some((f) => f.key === "jarPath");
                          setFieldValues((prev) => ({
                            ...prev,
                            jvmArgs: parsed.jvmArgs,
                            programArgs: parsed.programArgs,
                            ...(hasJarPathField ? { jarPath: parsed.jarPath } : {}),
                          }));
                          return;
                        }
                        // No "-jar" token, but a bare "java"/"java.exe" entry
                        // on its own is never a real JVM flag either (e.g.
                        // typed by hand into this field by mistake) - it
                        // would otherwise become an invalid positional
                        // argument java itself can't parse.
                        const cleaned = v.filter((token) => !JAVA_BINARY_TOKEN.test(token as string));
                        if (cleaned.length !== v.length) {
                          setFieldValue(field.key, cleaned);
                          return;
                        }
                      }
                      setFieldValue(field.key, v);
                    }}
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
                <Button variant="secondary" size="sm" onClick={() => setEnvironment((prev) => [...prev, { key: "", value: "", isSecret: false }])}>
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
                {selectedBlueprint.fields.map((field) => (
                  <Fragment key={field.key}>
                    <span className="wizard-review-label">{field.label}</span>
                    <span className="wizard-review-value">
                      {formatFieldValueForReview(field, fieldValueOrDefault(field, fieldValues), t("common.yes"), t("common.no"))}
                    </span>
                  </Fragment>
                ))}
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

/** flags.sh generates a ready-made set of JVM flags for a Minecraft server given its RAM/player count - opens through Tauri's shell plugin (a real system-browser navigation, not the app's own webview) rather than a plain `<a target="_blank">`, same as DatabasesTab's phpMyAdmin link. */
function JvmArgsGeneratorNote() {
  const { t } = useTranslation();
  return (
    <p className="form-note">
      {t("createApplicationWizard.jvmArgsGeneratorNote")}{" "}
      <button type="button" className="form-note-link" onClick={() => open("https://flags.sh/")}>
        flags.sh
      </button>
    </p>
  );
}

export interface BlueprintFieldInputProps {
  field: BlueprintField;
  value: unknown;
  onChange: (value: unknown) => void;
  serverId: string | null;
}

export function BlueprintFieldInput({ field, value, onChange, serverId }: BlueprintFieldInputProps) {
  if (field.fieldType === "javaVersion") {
    return <JavaVersionFieldInput field={field} value={value} onChange={onChange} serverId={serverId} />;
  }

  if (field.fieldType === "papermcVersion") {
    return <PapermcVersionFieldInput field={field} value={value} onChange={onChange} />;
  }

  if (field.fieldType === "boolean") {
    return (
      <div className="form-field">
        <Checkbox
          checked={Boolean(value)}
          onChange={onChange}
          label={
            <>
              {field.label}
              {field.required ? " *" : ""}
            </>
          }
        />
        {field.helpText && <p className="form-note">{field.helpText}</p>}
      </div>
    );
  }

  if (field.fieldType === "textList") {
    const text = Array.isArray(value) ? value.join("\n") : "";
    const isJvmArgs = field.key === "jvmArgs";
    return (
      <label className="form-field">
        <span className="form-label">
          {field.label}
          {field.required ? " *" : ""}
        </span>
        <textarea
          className="form-input form-textarea"
          value={text}
          onChange={(e) => onChange(splitTextListInput(e.target.value))}
          placeholder={isJvmArgs ? JVM_ARGS_PLACEHOLDER : undefined}
        />
        {field.helpText && <p className="form-note">{field.helpText}</p>}
        {isJvmArgs && <JvmArgsGeneratorNote />}
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
      <Select
        value={matchesDetected ? currentValue : ""}
        onChange={(value) => (value === "__custom__" ? setCustomPath(true) : onChange(value))}
        placeholder={t("createApplicationWizard.chooseJava")}
        items={[
          ...installations.map((installation) => ({
            value: installation.path,
            label: t("createApplicationWizard.javaOption", { major: installation.majorVersion }),
          })),
          { value: "__custom__", label: t("createApplicationWizard.customJavaPath") },
        ]}
      />
      {field.helpText && <p className="form-note">{field.helpText}</p>}
    </label>
  );
}

interface PapermcVersionFieldInputProps {
  field: BlueprintField;
  value: unknown;
  onChange: (value: unknown) => void;
}

/** Which project's release list this field's own key means - one entry per blueprint using this field type. Falls back to Paper's own list for any future field key that forgets to register here, same as the previous two-way check already did. */
const VERSION_FETCHERS: Record<string, () => Promise<string[]>> = {
  minecraftVersion: listPaperVersions,
  velocityVersion: listVelocityVersions,
  waterfallVersion: listWaterfallVersions,
  purpurVersion: listPurpurVersions,
};

function fetchVersionsFor(fieldKey: string): Promise<string[]> {
  return (VERSION_FETCHERS[fieldKey] ?? listPaperVersions)();
}

/** A picker populated from the real, current PaperMC release list for whichever project this field belongs to - falls back to a plain text input (with the load error, if any, shown rather than hidden) if the list couldn't be fetched at all, e.g. no network. */
function PapermcVersionFieldInput({ field, value, onChange }: PapermcVersionFieldInputProps) {
  const { t } = useTranslation();
  const [versions, setVersions] = useState<string[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);

  useEffect(() => {
    fetchVersionsFor(field.key)
      .then(setVersions)
      .catch((err) => {
        setVersions([]);
        setLoadError(err instanceof Error ? err.message : String(err));
      });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [field.key]);

  const currentValue = typeof value === "string" ? value : "";
  const label = (
    <span className="form-label">
      {field.label}
      {field.required ? " *" : ""}
    </span>
  );

  if (versions === null) {
    return (
      <label className="form-field">
        {label}
        <p className="form-note">{t("createApplicationWizard.loadingVersions")}</p>
      </label>
    );
  }

  if (versions.length === 0) {
    return (
      <label className="form-field">
        {label}
        <input className="form-input" value={currentValue} onChange={(e) => onChange(e.target.value)} placeholder="1.21.11" />
        {loadError && <p className="form-note form-note-danger">{t("createApplicationWizard.versionsLoadError", { error: loadError })}</p>}
        {field.helpText && <p className="form-note">{field.helpText}</p>}
      </label>
    );
  }

  return (
    <label className="form-field">
      {label}
      <Select
        value={currentValue}
        onChange={onChange}
        placeholder={t("createApplicationWizard.chooseVersion")}
        items={versions.map((version) => ({ value: version, label: version }))}
      />
      {field.helpText && <p className="form-note">{field.helpText}</p>}
    </label>
  );
}
