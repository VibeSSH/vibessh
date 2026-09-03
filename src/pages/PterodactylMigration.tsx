import { useEffect, useMemo, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { Link } from "react-router-dom";
import { Badge } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { Icon } from "@/components/ui/Icon";
import {
  connectPterodactyl,
  forgetPterodactyl,
  getPterodactylConnection,
  importPterodactyl,
  onPterodactylProgress,
  planPterodactylMigration,
} from "@/services/pterodactylService";
import { listDatabaseHosts } from "@/services/databaseService";
import { listServers } from "@/services/serverService";
import { errorMessage } from "@/services/tauri";
import type { DatabaseHost } from "@/types/database";
import type { ServerSummary } from "@/types/server";
import type { ImportOutcome, ImportProgress, NodeOverride, PlanNote, PlannedServer, PterodactylMigrationPlan } from "@/types/pterodactyl";
import "./pages.css";
import "./PterodactylMigration.css";

/**
 * Renders one plan note in the reader's language.
 *
 * Rust hands over a key and its parameters rather than a sentence, so that a
 * migration plan is not the one screen in this app that answers in English.
 * An unknown key falls back to the key itself, which is ugly on purpose:
 * a missing translation should look missing, not look like prose.
 */
function useNoteText() {
  const { t } = useTranslation();
  return (note: PlanNote) => t(`pterodactyl.notes.${note.code}`, { defaultValue: note.code, ...note.params });
}

/**
 * What this screen remembers between visits.
 *
 * A migration takes several attempts - a port collision, a machine that had
 * to be mapped by hand - and re-entering the panel address and the machine
 * map before each one is friction with no purpose. Deliberately not the API
 * key: that lives in the OS keyring and never reaches this side.
 */
const REMEMBERED = "vibessh.pterodactyl.setup";

interface RememberedSetup {
  baseUrl: string;
  nodeChoices: Record<string, string>;
  databaseHostId: string;
}

function loadRemembered(): RememberedSetup {
  const empty: RememberedSetup = { baseUrl: "", nodeChoices: {}, databaseHostId: "" };
  try {
    const stored = window.localStorage.getItem(REMEMBERED);
    if (!stored) return empty;
    const parsed: unknown = JSON.parse(stored);
    if (typeof parsed !== "object" || parsed === null) return empty;
    // Read defensively rather than trusted: this is storage a previous
    // version of this app wrote, and its shape is not guaranteed.
    const setup = parsed as Partial<RememberedSetup>;
    return {
      baseUrl: typeof setup.baseUrl === "string" ? setup.baseUrl : "",
      nodeChoices: typeof setup.nodeChoices === "object" && setup.nodeChoices !== null ? (setup.nodeChoices as Record<string, string>) : {},
      databaseHostId: typeof setup.databaseHostId === "string" ? setup.databaseHostId : "",
    };
  } catch {
    // Private browsing, cleared storage, or a value from a version that
    // wrote something else. An empty form is a fine outcome.
    return empty;
  }
}

/** The picker's map, in the shape the commands take. */
function asOverrides(choices: Record<string, string>): NodeOverride[] {
  return Object.entries(choices)
    .filter(([, serverId]) => serverId !== "")
    .map(([fqdn, serverId]) => ({ fqdn, serverId }));
}

/**
 * Setting up a migration from a Pterodactyl panel.
 *
 * **The plan is the product of this screen, not a formality before the real
 * one.** Every decision the importer would make - which VibeSSH image each
 * server becomes, which ports it keeps, which variables survive, where its
 * files are - is shown here with the reason for it, and nothing is created
 * until somebody has read that. A migration is the one operation where
 * finding out what was decided by looking at what already happened is
 * unacceptable: the source panel is usually being switched off afterwards.
 */
export function PterodactylMigration() {
  const { t } = useTranslation();
  const noteText = useNoteText();

  const remembered = useMemo(loadRemembered, []);
  const [baseUrl, setBaseUrl] = useState(remembered.baseUrl);
  const [apiKey, setApiKey] = useState("");
  const [hasStoredKey, setHasStoredKey] = useState(false);
  const [connecting, setConnecting] = useState(false);
  const [planning, setPlanning] = useState(false);
  const [plan, setPlan] = useState<PterodactylMigrationPlan | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [expanded, setExpanded] = useState<number | null>(null);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [running, setRunning] = useState(false);
  const [progress, setProgress] = useState<ImportProgress | null>(null);
  const [outcomes, setOutcomes] = useState<ImportOutcome[]>([]);
  const [databaseHosts, setDatabaseHosts] = useState<DatabaseHost[]>([]);
  const [databaseHostId, setDatabaseHostId] = useState(remembered.databaseHostId);
  const [servers, setServers] = useState<ServerSummary[]>([]);
  // fqdn -> VibeSSH server id. Kept here rather than derived from the plan,
  // because it is an answer about the world that the plan cannot know.
  const [nodeChoices, setNodeChoices] = useState<Record<string, string>>(remembered.nodeChoices);

  useEffect(() => {
    listServers()
      .then(setServers)
      .catch(() => setServers([]));
  }, []);

  useEffect(() => {
    try {
      window.localStorage.setItem(REMEMBERED, JSON.stringify({ baseUrl, nodeChoices, databaseHostId }));
    } catch {
      // Storage can be unavailable or full. The screen still works; it just
      // will not remember, which is what it did before this existed.
    }
  }, [baseUrl, nodeChoices, databaseHostId]);

  useEffect(() => {
    listDatabaseHosts()
      .then(setDatabaseHosts)
      .catch(() => setDatabaseHosts([]));
  }, []);

  // Subscribed once for the life of the page rather than per run: the
  // listener is cheap, and re-subscribing on every import is how events get
  // missed between the command starting and the handler attaching.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void onPterodactylProgress(setProgress).then((off) => {
      unlisten = off;
    });
    return () => unlisten?.();
  }, []);

  useEffect(() => {
    getPterodactylConnection()
      .then((connection) => setHasStoredKey(connection.hasStoredKey))
      .catch(() => setHasStoredKey(false));
  }, []);

  async function handleConnect(event: FormEvent) {
    event.preventDefault();
    setError(null);
    setConnecting(true);
    try {
      await connectPterodactyl(baseUrl, apiKey);
      setHasStoredKey(true);
      // Cleared the moment it is stored: there is no reason for an admin key
      // to sit in a React state field for the rest of the session.
      setApiKey("");
      await loadPlan();
    } catch (caught) {
      setError(caught);
    } finally {
      setConnecting(false);
    }
  }

  async function loadPlan() {
    setPlanning(true);
    setError(null);
    try {
      const loaded = await planPterodactylMigration(baseUrl, asOverrides(nodeChoices));
      setPlan(loaded);
      // Deliberately does not clear the outcomes. An import ends by rebuilding
      // the plan, and wiping the results here meant the summary of what just
      // happened vanished before anybody could read it.
      // Everything VibeSSH can actually reach starts ticked. A server whose
      // machine is unknown is left unticked rather than hidden: it is part of
      // the panel, and the reason it cannot move is on its own card.
      setSelected(new Set(loaded.servers.filter((server) => server.source.matchedServerId !== null).map((server) => server.sourceId)));
    } catch (caught) {
      setError(caught);
    } finally {
      setPlanning(false);
    }
  }

  function toggleSelected(sourceId: number) {
    setSelected((current) => {
      const next = new Set(current);
      if (next.has(sourceId)) {
        next.delete(sourceId);
      } else {
        next.add(sourceId);
      }
      return next;
    });
  }

  async function handleImport() {
    setRunning(true);
    setError(null);
    setProgress(null);
    // The previous run's results go now, at the start of this one, rather
    // than when the plan is next rebuilt.
    setOutcomes([]);
    try {
      setOutcomes(await importPterodactyl(baseUrl, [...selected], databaseHostId || undefined, asOverrides(nodeChoices)));
      // The panel has not changed, but this side has: rebuilding shows the
      // plan against what now exists rather than what did when it was built.
      await loadPlan();
    } catch (caught) {
      setError(caught);
    } finally {
      setRunning(false);
      setProgress(null);
    }
  }

  async function handleForget() {
    await forgetPterodactyl().catch(() => undefined);
    setHasStoredKey(false);
    setPlan(null);
  }

  // One row per machine, not per server: five servers on one node are one
  // question, and asking it five times would invite five different answers.
  const panelNodes = plan
    ? Object.values(
        plan.servers.reduce<Record<string, { fqdn: string; nodeName: string; matchedServerId: string | null }>>((byFqdn, server) => {
          const key = server.source.fqdn || server.source.nodeName;
          byFqdn[key] ??= { fqdn: key, nodeName: server.source.nodeName, matchedServerId: server.source.matchedServerId };
          return byFqdn;
        }, {}),
      )
    : [];

  return (
    <div className="page">
      <div className="page-header">
        <h1 className="page-title">{t("pterodactyl.title")}</h1>
        <p className="page-subtitle">{t("pterodactyl.subtitle")}</p>
      </div>

      <form className="card ptero-connect" onSubmit={handleConnect}>
        <label className="form-field">
          <span className="form-label">{t("pterodactyl.panelUrl")}</span>
          <input
            className="form-input"
            value={baseUrl}
            onChange={(event) => setBaseUrl(event.target.value)}
            placeholder="https://panel.example.com"
            required
          />
        </label>

        <label className="form-field">
          <span className="form-label">{t("pterodactyl.apiKey")}</span>
          <input
            className="form-input"
            type="password"
            value={apiKey}
            onChange={(event) => setApiKey(event.target.value)}
            placeholder={hasStoredKey ? t("pterodactyl.apiKeyStored") : "ptla_..."}
            required={!hasStoredKey}
          />
          <span className="form-help">{t("pterodactyl.apiKeyHelp")}</span>
        </label>

        <div className="ptero-actions">
          <Button type="submit" disabled={connecting || planning || baseUrl.trim() === ""}>
            <Icon name="wifi" size={14} />
            {connecting ? t("pterodactyl.connecting") : t("pterodactyl.connect")}
          </Button>
          {hasStoredKey && (
            <>
              <Button type="button" variant="secondary" onClick={() => void loadPlan()} disabled={planning || baseUrl.trim() === ""}>
                <Icon name="activity" size={14} />
                {planning ? t("pterodactyl.planning") : t("pterodactyl.rebuildPlan")}
              </Button>
              <Button type="button" variant="secondary" onClick={() => void handleForget()}>
                <Icon name="trash" size={14} />
                {t("pterodactyl.forgetKey")}
              </Button>
            </>
          )}
        </div>
      </form>

      {error != null && (
        <p className="form-note form-note-danger form-note-spaced" role="alert">
          {errorMessage(error, t)}
        </p>
      )}

      {plan && (
        <>
          <div className="ptero-summary card">
            <p className="ptero-summary-count">{t("pterodactyl.foundServers", { count: plan.servers.length })}</p>
            {plan.notes.map((note) => (
              <p key={note.code} className="ptero-note">
                <Icon name="alert-triangle" size={13} />
                {noteText(note)}
              </p>
            ))}
            {/* Said once, at the top, because it is the shape of the whole
              * operation rather than a property of any one server. */}
            <p className="ptero-note ptero-note-neutral">
              <Icon name="eye" size={13} />
              {t("pterodactyl.readOnlyNote")}
            </p>
          </div>

          {/* The machines. Shown whenever the plan has any node at all, not
            * only when one failed to match: an automatic match is a guess
            * from two addresses agreeing, and the operator is the only one
            * who actually knows. Getting it wrong copies files out of the
            * wrong server, so it is worth being able to see and correct. */}
          <div className="card ptero-machines">
            <p className="ptero-block-title">{t("pterodactyl.machines")}</p>
            <ul className="ptero-plain-list">
              {panelNodes.map((node) => (
                <li key={node.fqdn} className="ptero-machine">
                  <span className="ptero-machine-name">
                    {node.nodeName}
                    <span className="ptero-muted"> {node.fqdn}</span>
                  </span>
                  <select
                    className="form-input ptero-host-select"
                    value={nodeChoices[node.fqdn] ?? node.matchedServerId ?? ""}
                    onChange={(event) => setNodeChoices({ ...nodeChoices, [node.fqdn]: event.target.value })}
                    disabled={running || planning}
                    aria-label={t("pterodactyl.machineFor", { name: node.nodeName })}
                  >
                    <option value="">{t("pterodactyl.machineNone")}</option>
                    {servers.map((server) => (
                      <option key={server.id} value={server.id}>
                        {server.name} ({server.host})
                      </option>
                    ))}
                  </select>
                </li>
              ))}
            </ul>
            <p className="ptero-run-note">{t("pterodactyl.machinesNote")}</p>
            <Button type="button" variant="secondary" onClick={() => void loadPlan()} disabled={planning || running}>
              <Icon name="activity" size={14} />
              {t("pterodactyl.applyMachines")}
            </Button>
          </div>

          <div className="card ptero-run">
            <div className="ptero-run-left">
              <Button onClick={() => void handleImport()} disabled={running || selected.size === 0}>
                <Icon name="arrow-left-right" size={14} />
                {running ? t("pterodactyl.importing") : t("pterodactyl.importSelected", { count: selected.size })}
              </Button>
              <span className="ptero-run-note">{t("pterodactyl.leftStopped")}</span>
            </div>
            {/* Only shown when the panel actually has databases to move.
              * A picker for something nobody has is noise, and a required
              * answer to a question that does not apply is worse. */}
            {plan.servers.some((server) => server.databases.length > 0) &&
              (databaseHosts.length === 0 ? (
                // An empty dropdown is not an answer. Without a database host
                // there is nowhere for a schema to go, and saying so here -
                // where the choice would have been - beats a warning after
                // the import has already skipped them.
                <span className="ptero-note">
                  <Icon name="alert-triangle" size={13} />
                  {t("pterodactyl.noDatabaseHosts")}{" "}
                  <Link to="/database-hosts">{t("pterodactyl.noDatabaseHostsLink")}</Link>
                </span>
              ) : (
                <label className="ptero-run-hosts">
                  <span className="ptero-run-note">{t("pterodactyl.databaseTarget")}</span>
                  <select
                    className="form-input ptero-host-select"
                    value={databaseHostId}
                    onChange={(event) => setDatabaseHostId(event.target.value)}
                    disabled={running}
                  >
                    <option value="">{t("pterodactyl.databaseTargetNone")}</option>
                    {databaseHosts.map((host) => (
                      <option key={host.id} value={host.id}>
                        {host.name}
                      </option>
                    ))}
                  </select>
                </label>
              ))}
            {progress && (
              <span className="ptero-run-progress">
                {t("pterodactyl.progress", {
                  index: progress.index + 1,
                  total: progress.total,
                  name: progress.name,
                  step: t(`pterodactyl.steps.${progress.step}`),
                })}
              </span>
            )}
          </div>

          {outcomes.length > 0 && (
            <div className="card ptero-outcomes">
              <p className="ptero-block-title">{t("pterodactyl.results")}</p>
              <ul className="ptero-plain-list">
                {outcomes.map((outcome) => (
                  <li key={outcome.sourceId}>
                    {outcome.failed ? <Badge tone="danger">{t("pterodactyl.failed")}</Badge> : <Badge tone="success">{t("pterodactyl.moved")}</Badge>}{" "}
                    <strong>{outcome.name}</strong>{" "}
                    {/* Warnings raised *during* the import - a port that was
                      * already taken, a database that did not move - are
                      * only ever reported here. The plan's own card cannot
                      * know about them, and an unread warning about a
                      * missing database is the worst outcome this screen
                      * has. */}
                    {outcome.warnings.length > 0 && (
                      <ul className="ptero-warnings ptero-outcome-warnings">
                        {outcome.warnings.map((warning) => (
                          <li key={warning.code}>{noteText(warning)}</li>
                        ))}
                      </ul>
                    )}
                    <span className="ptero-muted">
                      {outcome.failed
                        ? noteText(outcome.failed)
                        : [
                            t("pterodactyl.filesCopied", { count: outcome.filesCopied }),
                            outcome.databasesMoved.length > 0 ? t("pterodactyl.databasesMoved", { count: outcome.databasesMoved.length }) : null,
                          ]
                            .filter(Boolean)
                            .join(", ")}
                    </span>
                  </li>
                ))}
              </ul>
            </div>
          )}

          <div className="ptero-list">
            {plan.servers.map((server) => (
              <PlannedServerCard
                key={server.sourceId}
                server={server}
                open={expanded === server.sourceId}
                onToggle={() => setExpanded(expanded === server.sourceId ? null : server.sourceId)}
                selected={selected.has(server.sourceId)}
                onSelect={() => toggleSelected(server.sourceId)}
                disabled={running}
              />
            ))}
          </div>
        </>
      )}
    </div>
  );
}

function PlannedServerCard({
  server,
  open,
  onToggle,
  selected,
  onSelect,
  disabled,
}: {
  server: PlannedServer;
  open: boolean;
  onToggle: () => void;
  selected: boolean;
  onSelect: () => void;
  disabled: boolean;
}) {
  const { t } = useTranslation();
  const noteText = useNoteText();
  const primary = server.ports.find((port) => port.primary) ?? server.ports[0];

  return (
    <div className="card ptero-server">
      <div className="ptero-server-row">
        <input
          type="checkbox"
          className="ptero-server-check"
          checked={selected}
          onChange={onSelect}
          disabled={disabled}
          aria-label={t("pterodactyl.selectServer", { name: server.name })}
        />
        <button type="button" className="ptero-server-head" onClick={onToggle} aria-expanded={open}>
        <div className="ptero-server-title">
          <span className="ptero-server-name">{server.name}</span>
          <span className="ptero-server-egg">{server.egg}</span>
        </div>
        <div className="ptero-server-meta">
          <Badge tone="neutral">{server.blueprintId}</Badge>
          {primary && <span className="ptero-server-port">:{primary.port}</span>}
          {server.databases.length > 0 && (
            <span className="ptero-server-chip">{t("pterodactyl.databaseCount", { count: server.databases.length })}</span>
          )}
          {server.warnings.length > 0 && (
            <Badge tone="warning">{t("pterodactyl.warningCount", { count: server.warnings.length })}</Badge>
          )}
            <Icon name={open ? "chevron-up" : "chevron-down"} size={15} />
          </div>
        </button>
      </div>

      {open && (
        <div className="ptero-server-body">
          <p className="ptero-reason">{t("pterodactyl.reason", { reason: noteText(server.reason) })}</p>

          <dl className="ptero-facts">
            <div>
              <dt>{t("pterodactyl.sourceMachine")}</dt>
              <dd>
                {server.source.nodeName}
                {server.source.matchedServerName ? (
                  <Badge tone="success">{t("pterodactyl.knownNode", { name: server.source.matchedServerName })}</Badge>
                ) : (
                  <Badge tone="warning">{t("pterodactyl.unknownNode")}</Badge>
                )}
              </dd>
            </div>
            <div>
              <dt>{t("pterodactyl.files")}</dt>
              <dd>
                <code>{server.source.volumePath || "-"}</code>
              </dd>
            </div>
            <div>
              <dt>{t("pterodactyl.ports")}</dt>
              <dd>{server.ports.length === 0 ? "-" : server.ports.map((port) => port.port).join(", ")}</dd>
            </div>
            <div>
              <dt>{t("pterodactyl.limits")}</dt>
              <dd>
                {server.memoryMb === null && server.cpuCores === null
                  ? t("pterodactyl.noLimits")
                  : [
                      server.memoryMb !== null ? `${server.memoryMb} MB` : null,
                      server.cpuCores !== null ? t("pterodactyl.cores", { count: server.cpuCores }) : null,
                    ]
                      .filter(Boolean)
                      .join(" / ")}
              </dd>
            </div>
          </dl>

          {server.databases.length > 0 && (
            <div className="ptero-block">
              <p className="ptero-block-title">{t("pterodactyl.databases")}</p>
              <ul className="ptero-plain-list">
                {server.databases.map((database) => (
                  <li key={database.name}>
                    <code>{database.name}</code> <span className="ptero-muted">{database.username}</span>{" "}
                    {database.hostServerName ? (
                      <span className="ptero-muted">{t("pterodactyl.onMachine", { name: database.hostServerName })}</span>
                    ) : (
                      <Badge tone="warning">{database.hostAddress || t("pterodactyl.unknownMachine")}</Badge>
                    )}
                  </li>
                ))}
              </ul>
            </div>
          )}

          {server.environment.length > 0 && (
            <div className="ptero-block">
              <p className="ptero-block-title">{t("pterodactyl.environment")}</p>
              <ul className="ptero-plain-list">
                {server.environment.map((variable) => (
                  <li key={variable.key}>
                    <code>{variable.key}</code> <span className="ptero-muted">{variable.value}</span>
                  </li>
                ))}
              </ul>
            </div>
          )}

          {server.warnings.length > 0 && (
            <div className="ptero-block">
              <p className="ptero-block-title">{t("pterodactyl.warnings")}</p>
              <ul className="ptero-warnings">
                {server.warnings.map((warning) => (
                  <li key={warning.code}>{noteText(warning)}</li>
                ))}
              </ul>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
