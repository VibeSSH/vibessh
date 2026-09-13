import { Fragment, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { open } from "@tauri-apps/plugin-shell";
import { Button } from "@/components/ui/Button";
import { deleteApplicationTemplate, listApplicationTemplates, saveApplicationTemplate } from "@/services/applicationTemplateService";
import type { ApplicationTemplate } from "@/types/applicationTemplate";
import { Checkbox } from "@/components/ui/Checkbox";
import { HelpHint } from "@/components/ui/HelpHint";
import { Icon } from "@/components/ui/Icon";
import { Select } from "@/components/ui/Select";
import { IconButton } from "@/components/ui/IconButton";
import { useModalDialog } from "@/hooks/useModalDialog";
import { createApplication, detectJavaInstallations, listApplications, listPaperVersions, listPurpurVersions, listVelocityVersions, listWaterfallVersions } from "@/services/applicationService";
import { installDocker, listServers, probeServerCapabilities, serverSummaryToManagedServer } from "@/services/serverService";
import { listDatabaseHosts } from "@/services/databaseService";
import { localApplicationsRoot, localDockerAvailable } from "@/services/appService";
import type { DatabaseHost } from "@/types/database";
import { reachableDatabaseAddress } from "@/utils/databaseAddress";
import { useServersStore } from "@/stores/serversStore";
import { toastSuccess } from "@/stores/toastStore";
import { translateBlueprint, translateTemplateName } from "@/i18n/blueprintTranslations";
import type { Application, Blueprint, BlueprintField, EnvironmentVariable, JavaInstallation, RuntimeType } from "@/types/application";
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
export function runtimeTypesForLocation(blueprint: Blueprint, isLocal: boolean): RuntimeType[] {
  // Docker is offered locally as well as remotely. It used to be remote-only,
  // not by design but because the Docker runtime spoke POSIX shell over SSH -
  // every invocation a `sudo docker ...` string. It now builds arguments and
  // hands them to whichever daemon the Application belongs to, and an
  // Application with no Node is exactly the one that means this machine.
  //
  // `localProcess` stays the other way round: it runs a program here, so it
  // has nothing to say about a remote Node.
  const usable = blueprint.supportedRuntimeTypes.filter((rt) => (isLocal ? rt === "localProcess" || rt === "docker" : rt !== "localProcess"));
  if (!isLocal) return usable;
  // Locally, the plain process comes first.
  //
  // Blueprints list Docker first because that is the runtime they were
  // written for, and the list was shown in that order - so on Windows the
  // option somebody reached for was the one that needs Docker Desktop, WSL2
  // and a reboot, when the other one needs nothing at all and downloads its
  // own Java. Ordering is the whole intervention: Docker stays available and
  // stays chooseable, it just stops being the first thing offered on a
  // machine where it is the harder of the two.
  return [...usable].sort((a, b) => (a === "localProcess" ? -1 : b === "localProcess" ? 1 : 0));
}

/**
 * Something a `connectsTo` blueprint can be pointed at.
 *
 * Two shapes, because the two are answered differently. An Application is
 * named by id and the backend resolves its network alias and grants the
 * connection that makes it reachable. A Database Host has no container to
 * connect to and no alias - it is reached at a plain address, which is
 * filled straight into the environment where the user can see it before
 * anything is created.
 */
interface ConnectionTarget {
  value: string;
  label: string;
  applicationId?: string;
  address?: { host: string; port: number };
}

/**
 * The environment rows a chosen Database Host target adds.
 *
 * Only for a host: an Application target is resolved on the Rust side, where
 * its network alias is known. A row the user typed themselves always wins -
 * somebody who has already put a PMA_HOST in meant it.
 *
 * Exported for the test that pins the behaviour this exists for: the address
 * a container needs is not the address the database host is configured with.
 */
export function withConnectionEnvironment(
  environment: EnvironmentVariable[],
  blueprint: Blueprint,
  target: { address?: { host: string; port: number } } | null,
): EnvironmentVariable[] {
  const connection = blueprint.connectsTo;
  if (!connection || !target?.address) return environment;

  const rows = [...environment];
  const missing = (key: string) => !rows.some((row) => row.key === key);
  if (missing(connection.hostEnv)) rows.push({ key: connection.hostEnv, value: target.address.host, isSecret: false });
  if (missing(connection.portEnv)) rows.push({ key: connection.portEnv, value: String(target.address.port), isSecret: false });
  return rows;
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
  const [templates, setTemplates] = useState<ApplicationTemplate[]>([]);
  /**
   * Every Application, for a blueprint that points at one of them.
   *
   * Loaded once alongside the blueprints rather than when the picker
   * appears: by then somebody is three steps in and waiting, and this is a
   * local database read.
   */
  const [existingApplications, setExistingApplications] = useState<Application[]>([]);
  /**
   * The other kind of target.
   *
   * A database made on an application's Databases tab lives on a Database
   * Host, not in a MariaDB Application - which is where the reported
   * failures come from. Offering only Applications would leave exactly the
   * people VibeSSH created a database for with nothing to pick.
   */
  const [databaseHosts, setDatabaseHosts] = useState<DatabaseHost[]>([]);
  /**
   * Whether this machine can run containers.
   *
   * `null` while the answer is still being fetched, and `null` outside a
   * Tauri window - neither is a reason to warn, the same distinction the
   * Node-side check below draws between "no" and "not known yet".
   */
  const [localDocker, setLocalDocker] = useState<boolean | null>(null);
  /** Where local applications go by default - see `localApplicationsRoot`. */
  const [localRoot, setLocalRoot] = useState<string | null>(null);
  /** Prefixed, because the two kinds of target are answered differently: `app:<id>` or `host:<id>`. */
  const [connectToId, setConnectToId] = useState("");
  // Only ever the name being typed into the save box - null while it is shut.
  const [templateName, setTemplateName] = useState<string | null>(null);
  /**
   * Which template the wizard was filled in from, if any.
   *
   * Cleared the moment the answers stop being that template's - picking a
   * different application type by hand means the form no longer holds what
   * the template said, and a row still marked as chosen would be claiming
   * otherwise.
   */
  const [appliedTemplateId, setAppliedTemplateId] = useState<string | null>(null);
  const [templateError, setTemplateError] = useState<string | null>(null);
  /**
   * Whether a create is already in flight.
   *
   * `busy` disables the button, but only once React has re-rendered, and a
   * held-down Enter repeats faster than that - which is how somebody ended
   * up with a dozen identical Nodes from one save. A ref is not state, so
   * the guard sees what the line above it set.
   */
  const creating = useRef(false);
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
    // Absent outside a Tauri webview, and an empty list is the correct
    // state for anyone who has never saved one.
    listApplicationTemplates()
      .then(setTemplates)
      .catch(() => undefined);
    listApplications()
      .then(setExistingApplications)
      .catch(() => undefined);
    listDatabaseHosts()
      .then(setDatabaseHosts)
      .catch(() => undefined);
    localDockerAvailable()
      .then(setLocalDocker)
      .catch(() => undefined);
    localApplicationsRoot()
      .then(setLocalRoot)
      .catch(() => undefined);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const isLocal = serverId === null;

  /**
   * Fills the wizard in from a saved template.
   *
   * Everything except where it runs. That is the one answer that genuinely
   * differs each time, and prefilling it would put the previous server in
   * front of somebody who opened the template precisely to use a different
   * one - so the location stays whatever this session already chose.
   *
   * A secret variable arrives with its name and no value, because none was
   * ever saved. It lands in the list as an empty secret row, which is what
   * step 4 already renders as "still needs filling in".
   */
  function applyTemplate(template: ApplicationTemplate) {
    setAppliedTemplateId(template.id);
    setBlueprintId(template.blueprintId);
    setRuntimeType(template.runtimeType);
    setFieldValues({ ...template.fieldValues });
    setEnvironment(template.environment.map((variable) => ({ key: variable.key, value: variable.value, isSecret: variable.isSecret })));
    setError(null);
  }

  async function handleSaveTemplate() {
    if (!blueprintId || !runtimeType) return;
    setTemplateError(null);
    try {
      const saved = await saveApplicationTemplate({
        id: crypto.randomUUID(),
        name: templateName ?? "",
        blueprintId,
        runtimeType,
        fieldValues,
        // Secret values are dropped again on the Rust side before anything is
        // written - this is convenience, not the guarantee.
        environment: environment.map((variable) => ({
          key: variable.key,
          value: variable.isSecret ? "" : variable.value,
          isSecret: variable.isSecret,
        })),
        createdAt: new Date().toISOString(),
        // Whatever the wizard was filled in from, what is being saved here is
        // the user's own - the backend enforces the same thing.
        isBuiltin: false,
      });
      setTemplates((previous) => [...previous, saved]);
      setTemplateName(null);
      toastSuccess(t("createApplicationWizard.templateSaved", { name: saved.name }));
    } catch (err) {
      setTemplateError(errorMessage(err, t));
    }
  }

  async function handleDeleteTemplate(templateId: string) {
    try {
      await deleteApplicationTemplate(templateId);
      setTemplates((previous) => previous.filter((template) => template.id !== templateId));
      // The answers stay - somebody deleting a template mid-wizard is
      // tidying up, not starting over - but nothing is left to point at.
      setAppliedTemplateId((current) => (current === templateId ? null : current));
    } catch (err) {
      setTemplateError(errorMessage(err, t));
    }
  }

  // A Pterodactyl-style suggested path, not a requirement - a remote
  // application always runs inside its own bind-mounted Docker directory
  // (or a plain SSH working directory for non-Docker runtimes) either way,
  // so a sensible default beats an empty required field. Stops suggesting
  // the moment the user edits the field themselves; Local has no such
  // convention to suggest, so it's left for the placeholder text alone.
  useEffect(() => {
    if (workingDirectoryTouched) return;
    if (!isLocal) {
      setWorkingDirectory(`/home/container/${slugify(name)}`);
      return;
    }
    // Local used to be left empty, so the first thing anybody did on Windows
    // was invent a path - and what people invent is the Desktop, which is
    // where a server then writes its worlds and logs. The separator comes
    // from the root itself rather than from a guess about the platform.
    if (!localRoot) return;
    const separator = localRoot.includes("\\") ? "\\" : "/";
    setWorkingDirectory(`${localRoot}${separator}${slugify(name)}`);
  }, [name, isLocal, workingDirectoryTouched, localRoot]);

  const selectedBlueprint = useMemo(() => blueprints.find((b) => b.id === blueprintId) ?? null, [blueprints, blueprintId]);

  /**
   * Applications this one could be pointed at.
   *
   * Same Node and Docker, because that is what a granted connection can
   * actually be implemented as - a Docker network does not span hosts, and a
   * bare process is not on one. Offering an unreachable target would produce
   * an application that looks configured and connects to nothing, which is
   * the failure this whole picker exists to remove.
   */
  const connectionTargets = useMemo<ConnectionTarget[]>(() => {
    const connection = selectedBlueprint?.connectsTo;
    if (!connection) return [];

    const applications = existingApplications
      .filter(
        (candidate) =>
          candidate.runtimeType === "docker" &&
          (candidate.serverId ?? null) === serverId &&
          (connection.blueprintIds.length === 0 || connection.blueprintIds.includes(candidate.blueprintId)),
      )
      .map((candidate) => ({ value: `app:${candidate.id}`, label: candidate.name, applicationId: candidate.id }));

    // Same location, for the same reason the applications are: a container
    // on one Node cannot reach a database server sitting on another one's
    // loopback address.
    const hosts = databaseHosts
      .filter((host) => (host.serverId ?? null) === serverId)
      .map((host) => {
        const address = reachableDatabaseAddress(host);
        return { value: `host:${host.id}`, label: `${host.name} (${address.host}:${address.port})`, address };
      });

    return [...applications, ...hosts];
  }, [selectedBlueprint, existingApplications, databaseHosts, serverId]);

  const chosenTarget = connectionTargets.find((target) => target.value === connectToId) ?? null;
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

  // The same warning for this machine, which had none: choosing Docker
  // locally used to succeed through the whole wizard and fail on the first
  // start with an error about PATH. There is no one-click install to offer
  // here - Docker Desktop is a download and, on Windows, a reboot - so the
  // message says what to get instead of offering a button that cannot exist.
  const localDockerWarning = isLocal && runtimeType === "docker" && localDocker === false;

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
    // Read in the same tick it is written, unlike `busy` - see the ref's own
    // comment. A repeat here is worse than a duplicate Node: each one
    // provisions, which for a Minecraft blueprint means downloading a server
    // jar again.
    if (creating.current) return;
    creating.current = true;
    setBusy(true);
    setError(null);
    try {
      await createApplication({
        serverId: serverId ?? undefined,
        name: name.trim(),
        workingDirectory: workingDirectory.trim(),
        blueprintId: selectedBlueprint.id,
        runtimeType,
        environment: withConnectionEnvironment(environment, selectedBlueprint, chosenTarget).filter((row) => row.key.trim().length > 0),
        blueprintInputs: Object.fromEntries(selectedBlueprint.fields.map((field) => [field.key, fieldValueOrDefault(field, fieldValues)])),
        // Only an Application target goes to the backend: it is the one whose
        // address the frontend cannot know (a network alias) and the one that
        // needs a connection granted. Nothing chosen is a real answer too - a
        // phpMyAdmin pointed at a database VibeSSH does not manage.
        connectToApplicationId: chosenTarget?.applicationId,
      });
      onCreated();
    } catch (err) {
      setError(errorMessage(err, t));
    } finally {
      creating.current = false;
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
                {/* Absent entirely until something has been saved: an empty
                    list here would be a permanent explanation of a feature
                    nobody had used yet. */}
                {templates.length > 0 && (
                  <div className="form-field">
                    <span className="form-label">{t("createApplicationWizard.templates")}</span>
                    <div className="wizard-template-list">
                      {templates.map((template) => {
                        const label = translateTemplateName(template, i18n.language);
                        const applied = template.id === appliedTemplateId;
                        return (
                          <div key={template.id} className="wizard-template">
                            <button
                              type="button"
                              className={`wizard-template-use ${applied ? "wizard-template-use-applied" : ""}`}
                              // Says which one is in use to a screen reader as
                              // well as to the eye - the tick alone is a
                              // picture.
                              aria-pressed={applied}
                              onClick={() => applyTemplate(template)}
                            >
                              <Icon name={template.isBuiltin ? "box" : "copy"} size={14} />
                              <span>{label}</span>
                              {applied && <Icon name="check" size={14} className="wizard-template-check" />}
                            </button>
                            {/* No delete on a built-in: it ships with the app,
                                the backend refuses to remove it, and a button
                                that always errors is worse than none. */}
                            {!template.isBuiltin && (
                              <IconButton
                                icon="trash"
                                size="sm"
                                onClick={() => void handleDeleteTemplate(template.id)}
                                title={t("createApplicationWizard.templateDelete", { name: label })}
                              />
                            )}
                          </div>
                        );
                      })}
                    </div>
                    <span className="form-hint">{t("createApplicationWizard.templatesHint")}</span>
                  </div>
                )}
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
                        onClick={() => {
                          setBlueprintId(blueprint.id);
                          // Picking a type by hand is the point where the
                          // form stops being whatever template filled it in.
                          setAppliedTemplateId(null);
                        }}
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
                {/* Only when both are on offer, which is only ever locally:
                    on a Node there is nothing to weigh up. */}
                {isLocal && availableRuntimeTypes.length > 1 && (
                  <p className="form-note">{t("createApplicationWizard.localRuntimeHint")}</p>
                )}
                {selectedBlueprint && availableRuntimeTypes.length === 0 && (
                  <p className="form-note form-note-danger">{t("createApplicationWizard.noRuntimeForLocation")}</p>
                )}
                {localDockerWarning && (
                  <div className="wizard-docker-warning">
                    <p className="form-note form-note-danger">{t("createApplicationWizard.localDockerMissing")}</p>
                    <p className="form-note">
                      {t("createApplicationWizard.localDockerGet")}{" "}
                      <button type="button" className="form-note-link" onClick={() => open("https://www.docker.com/products/docker-desktop/")}>
                        docker.com
                      </button>
                    </p>
                    {/* The way out that needs nothing installed. Worth saying
                        here rather than leaving somebody to reboot twice
                        before discovering it. */}
                    <p className="form-note">{t("createApplicationWizard.localDockerAlternative")}</p>
                  </div>
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
                {/* Before the blueprint's own fields, because it is the
                    question this kind of application exists to answer. What
                    it sets - the host, the port, and the granted connection
                    that makes the host resolvable at all - is spelled out
                    rather than left to happen quietly: all three are things
                    people currently go looking for by hand. */}
                {selectedBlueprint.connectsTo && (
                  <label className="form-field">
                    <span className="form-label">{t("createApplicationWizard.connectTo")}</span>
                    {connectionTargets.length === 0 ? (
                      <p className="form-note">{t("createApplicationWizard.connectToNone")}</p>
                    ) : (
                      <>
                        <Select
                          value={connectToId}
                          onChange={setConnectToId}
                          placeholder={t("createApplicationWizard.connectToNothing")}
                          items={connectionTargets.map((candidate) => ({ value: candidate.value, label: candidate.label }))}
                        />
                        {/* Three different sentences, because three different
                            things happen. A container target needs a granted
                            connection; a database host needs an address that
                            is not the one it is configured with; picking
                            nothing leaves the variable unset, which is the
                            state people arrive here already stuck in. */}
                        <span className="form-hint">
                          {!chosenTarget
                            ? t("createApplicationWizard.connectToHintNone", { host: selectedBlueprint.connectsTo.hostEnv })
                            : chosenTarget.address
                              ? t("createApplicationWizard.connectToHintHost", {
                                  host: selectedBlueprint.connectsTo.hostEnv,
                                  address: chosenTarget.address.host,
                                  port: chosenTarget.address.port,
                                })
                              : t("createApplicationWizard.connectToHint", {
                                  host: selectedBlueprint.connectsTo.hostEnv,
                                  port: selectedBlueprint.connectsTo.portEnv,
                                })}
                        </span>
                      </>
                    )}
                  </label>
                )}
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

                <div className="wizard-template-save">
                  {templateName === null ? (
                    <Button variant="secondary" size="sm" onClick={() => setTemplateName("")}>
                      <Icon name="copy" size={14} />
                      {t("createApplicationWizard.saveAsTemplate")}
                    </Button>
                  ) : (
                    <>
                      <input
                        className="form-input"
                        value={templateName}
                        onChange={(e) => setTemplateName(e.target.value)}
                        placeholder={t("createApplicationWizard.templateNamePlaceholder")}
                        aria-label={t("createApplicationWizard.templateNamePlaceholder")}
                        autoFocus
                      />
                      <Button size="sm" onClick={() => void handleSaveTemplate()} disabled={templateName.trim().length === 0}>
                        {t("common.save")}
                      </Button>
                      <Button variant="secondary" size="sm" onClick={() => { setTemplateName(null); setTemplateError(null); }}>
                        {t("common.cancel")}
                      </Button>
                    </>
                  )}
                </div>
                {/* Said next to the button rather than in a help page: it is
                    the one thing about a template that will surprise somebody
                    who saved one with a password in it. */}
                <p className="form-hint wizard-template-note">{t("createApplicationWizard.templateSecretNote")}</p>
                {templateError && <p className="form-note form-note-danger">{templateError}</p>}
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
