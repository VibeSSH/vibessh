import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { Checkbox } from "@/components/ui/Checkbox";
import { Dialog } from "@/components/ui/Dialog";
import { Icon } from "@/components/ui/Icon";
import { Select } from "@/components/ui/Select";
import { createApplication } from "@/services/applicationService";
import { scanForServers, type DiscoveredServer } from "@/services/serverDiscoveryService";
import { errorMessage } from "@/services/tauri";
import { toastSuccess } from "@/stores/toastStore";
import type { ManagedServer } from "@/stores/serversStore";
import "./AdoptServersModal.css";

interface AdoptServersModalProps {
  servers: ManagedServer[];
  onClose: () => void;
  /** Called once anything was created, so the list behind can refresh. */
  onAdopted: () => void;
}

/** Where a Pterodactyl host keeps them, which is where most of these live. */
const DEFAULT_REMOTE_DIRECTORY = "/home/container";

/**
 * Adopts game servers that already exist on a machine.
 *
 * **Why this exists.** The files arrive long before VibeSSH hears about them:
 * a Pterodactyl install left behind, a server somebody has been running by
 * hand, a folder restored from a backup. Adopting one meant the five-step
 * wizard and retyping what the directory already says - its name, its jar,
 * its port. Four servers meant four passes.
 *
 * **What it creates.** A plain Docker Application per server, pointed at the
 * directory as it is. Deliberately not Paper or Velocity even when the jar
 * says so: those manage the server's version, and managing it starts by
 * downloading a different jar into a directory somebody is already running a
 * server out of. Adopting something must not quietly replace it. Switching a
 * server to a managed blueprint afterwards is a decision somebody can make
 * once they can see it.
 */
export function AdoptServersModal({ servers, onClose, onAdopted }: AdoptServersModalProps) {
  const { t } = useTranslation();
  const [serverId, setServerId] = useState<string>("");
  const [directory, setDirectory] = useState(DEFAULT_REMOTE_DIRECTORY);
  const [found, setFound] = useState<DiscoveredServer[] | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const isLocal = serverId === "";

  async function handleScan() {
    setBusy(true);
    setError(null);
    setFound(null);
    try {
      const results = await scanForServers(isLocal ? null : serverId, directory);
      setFound(results);
      // Everything ticked: somebody who pointed this at a directory of
      // servers wants the servers in it, and unticking one is less work than
      // ticking six.
      setSelected(new Set(results.map((server) => server.path)));
    } catch (err) {
      setError(errorMessage(err, t));
    } finally {
      setBusy(false);
    }
  }

  async function handleAdopt() {
    if (!found) return;
    const chosen = found.filter((server) => selected.has(server.path));
    if (chosen.length === 0) return;

    setBusy(true);
    setError(null);
    let created = 0;
    try {
      // One at a time and in order, so a failure part way through leaves a
      // knowable state: everything before it exists, everything after it does
      // not, and the message names what stopped it.
      for (const server of chosen) {
        await createApplication({
          serverId: isLocal ? undefined : serverId,
          name: server.name,
          blueprintId: "generic-docker",
          runtimeType: "docker",
          workingDirectory: server.path,
          environment: [],
          blueprintInputs: {
            image: "eclipse-temurin:21-jre",
            // The jar that is already there, run the way a server is run.
            // Memory is left to the image's default rather than guessed at:
            // a wrong -Xmx is worse than none.
            command: ["java", "-jar", server.jar, "nogui"],
          },
        });
        created += 1;
      }
      toastSuccess(t("adoptServers.createdToast", { count: created }));
      onAdopted();
      onClose();
    } catch (err) {
      setError(t("adoptServers.partialError", { created, error: errorMessage(err, t) }));
      onAdopted();
    } finally {
      setBusy(false);
    }
  }

  function toggle(path: string) {
    setSelected((previous) => {
      const next = new Set(previous);
      if (!next.delete(path)) next.add(path);
      return next;
    });
  }

  return (
    <Dialog open onClose={onClose} title={t("adoptServers.title")} size="lg">
      <div className="modal-body">
        <p className="dialog-body-text">{t("adoptServers.intro")}</p>

        <div className="adopt-servers-where">
          <label className="form-field">
            <span className="form-label">{t("adoptServers.location")}</span>
            <Select
              value={serverId}
              onChange={(value) => {
                setServerId(value);
                setFound(null);
                // The default only makes sense on a Node; a local machine has
                // no `/home/container`.
                setDirectory(value === "" ? "" : DEFAULT_REMOTE_DIRECTORY);
              }}
              items={[
                { value: "", label: t("adoptServers.locationLocal") },
                ...servers.map((server) => ({ value: server.id, label: server.name })),
              ]}
            />
          </label>

          <label className="form-field adopt-servers-directory">
            <span className="form-label">{t("adoptServers.directory")}</span>
            <input
              className="form-input"
              value={directory}
              onChange={(event) => setDirectory(event.target.value)}
              placeholder={isLocal ? t("adoptServers.directoryPlaceholderLocal") : DEFAULT_REMOTE_DIRECTORY}
            />
          </label>

          <Button onClick={() => void handleScan()} disabled={busy || directory.trim().length === 0}>
            <Icon name="search" size={14} />
            {busy && found === null ? t("adoptServers.scanning") : t("adoptServers.scan")}
          </Button>
        </div>

        {found !== null && found.length === 0 && <p className="form-note">{t("adoptServers.nothingFound")}</p>}

        {found !== null && found.length > 0 && (
          <ul className="adopt-servers-list">
            {found.map((server) => (
              <li key={server.path} className="adopt-servers-row">
                <Checkbox checked={selected.has(server.path)} onChange={() => toggle(server.path)} label={null} />
                <div className="adopt-servers-name">
                  <span>{server.name}</span>
                  {server.kind !== "unknown" && <span className="adopt-servers-kind">{server.kind}</span>}
                </div>
                <span className="adopt-servers-jar" title={server.jar}>
                  {server.jar}
                </span>
                {/* Said out loud rather than left blank: a proxy keeps its
                    port elsewhere, and an empty column reads as a failure to
                    read one. */}
                <span className="adopt-servers-port">
                  {server.port === null ? t("adoptServers.noPort") : server.port}
                </span>
              </li>
            ))}
          </ul>
        )}

        {found !== null && found.length > 0 && <p className="form-hint">{t("adoptServers.adoptNote")}</p>}
        {error && <p className="form-note form-note-danger form-note-spaced">{error}</p>}

        <div className="form-actions">
          <Button variant="secondary" onClick={onClose} disabled={busy}>
            {t("common.cancel")}
          </Button>
          <Button onClick={() => void handleAdopt()} disabled={busy || selected.size === 0}>
            {t("adoptServers.adopt", { count: selected.size })}
          </Button>
        </div>
      </div>
    </Dialog>
  );
}
