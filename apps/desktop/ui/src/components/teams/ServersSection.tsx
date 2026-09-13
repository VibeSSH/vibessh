import { FormEvent, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { EmptyState } from "@/components/ui/EmptyState";
import { HostAddress } from "@/components/ui/HostAddress";
import { Icon } from "@/components/ui/Icon";
import { IconButton } from "@/components/ui/IconButton";
import { SkeletonRows } from "@/components/ui/SkeletonRows";
import {
  cloudCreateServer,
  cloudDeleteServer,
  cloudListServers,
  listPendingRevocations,
  syncTeamNodeAccess,
  type NodeAccessSync,
  type NodeRevocation,
} from "@/services/cloudService";
import { listServers } from "@/services/serverService";
import type { ServerSummary } from "@/types/server";
import { toastSuccess } from "@/stores/toastStore";
import type { CloudServer } from "@/types/cloud";
import "./ServersSection.css";
import { errorMessage } from "@/services/tauri";

export function ServersSection({ teamId, canManage }: { teamId: string; canManage: boolean }) {
  const { t } = useTranslation();
  const [servers, setServers] = useState<CloudServer[]>([]);
  // This install's own servers, which is what "grant access" actually runs
  // against. A team server is metadata; the account has to be created over a
  // connection somebody already has, and only this machine has one.
  const [localServers, setLocalServers] = useState<ServerSummary[]>([]);
  const [granting, setGranting] = useState<string | null>(null);
  const [grantResults, setGrantResults] = useState<Record<string, NodeAccessSync>>({});
  /**
   * Access the team has taken away that is still on a Node.
   *
   * Held here rather than derived from the last sync, because the install
   * that removed somebody is usually not the one that can reach the machine.
   * On this screen it is the difference between "they are gone" and "we
   * asked for them to be gone", and only one of those is true until a sync
   * runs.
   */
  const [pending, setPending] = useState<NodeRevocation[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [name, setName] = useState("");
  const [host, setHost] = useState("");
  const [sshPort, setSshPort] = useState("22");
  const [username, setUsername] = useState("");
  const [saving, setSaving] = useState(false);

  function load() {
    setLoading(true);
    setError(null);
    cloudListServers(teamId)
      .then(setServers)
      .catch((err) => setError(errorMessage(err, t)))
      .finally(() => setLoading(false));
  }

  useEffect(load, [teamId]); // eslint-disable-line react-hooks/exhaustive-deps

  function loadPending() {
    listPendingRevocations(teamId)
      .then(setPending)
      .catch(() => setPending([]));
  }

  useEffect(loadPending, [teamId]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    listServers()
      .then(setLocalServers)
      .catch(() => setLocalServers([]));
  }, []);

  /**
   * The local server that is this team server, matched on address.
   *
   * Matched rather than linked because nothing records the pair: a team
   * server is a name and an address, and each install adds the machine
   * separately. Host and port together are what identifies a machine to SSH,
   * so they are what identifies it here.
   */
  function localMatch(server: CloudServer): ServerSummary | undefined {
    return localServers.find((local) => local.host === server.host && local.sshPort === server.sshPort);
  }

  async function handleSync(server: CloudServer) {
    const local = localMatch(server);
    if (!local) return;
    setGranting(server.id);
    setError(null);
    try {
      const result = await syncTeamNodeAccess(local.id, teamId, server.id);
      setGrantResults((current) => ({ ...current, [server.id]: result }));
      const granted = result.members.filter((member) => member.granted).length;
      const revoked = result.revocations.filter((revocation) => revocation.completed).length;
      toastSuccess(t("teamServers.syncedToast", { granted, revoked }));
      // Whatever did not land is still owed, so the list is re-read rather
      // than adjusted here: the backend is what knows, and a local guess
      // could show somebody as removed when their key is still in place.
      loadPending();
    } catch (err) {
      setError(errorMessage(err, t));
    } finally {
      setGranting(null);
    }
  }

  async function handleCreate(e: FormEvent) {
    e.preventDefault();
    if (!name.trim() || !host.trim()) return;
    setSaving(true);
    setError(null);
    try {
      await cloudCreateServer(teamId, name.trim(), host.trim(), Number(sshPort) || 22, username.trim() || null);
      setName("");
      setHost("");
      setSshPort("22");
      setUsername("");
      toastSuccess(t("teamServers.addedToast", { name: name.trim() }));
      load();
    } catch (err) {
      setError(errorMessage(err, t));
    } finally {
      setSaving(false);
    }
  }

  /** What this Node still owes, which is what makes the warning specific. */
  function pendingFor(server: CloudServer): NodeRevocation[] {
    return pending.filter((revocation) => revocation.teamServerId === server.id);
  }

  async function handleDelete(server: CloudServer) {
    setError(null);
    try {
      await cloudDeleteServer(teamId, server.id);
      toastSuccess(t("teamServers.removedToast", { name: server.name }));
      load();
    } catch (err) {
      setError(errorMessage(err, t));
    }
  }

  return (
    <Card title={t("teamServers.title")} subtitle={t("teamServers.subtitle")}>
      {error && <p className="page-error-note">{error}</p>}

      {/* Said where the action is, not in a document nobody opens. A member's
          account can do everything the app can do on that Node - the account
          is theirs and the Node's log names them, but it is not a smaller
          set of powers until role-derived sudo lands. */}
      {canManage && <p className="team-servers-privilege-note">{t("teamServers.privilegeNote")}</p>}

      {loading ? (
        <SkeletonRows />
      ) : servers.length === 0 ? (
        <EmptyState icon="server" title={t("teamServers.emptyTitle")} description={t("teamServers.emptyDescription")} />
      ) : (
        <ul className="server-list">
          {servers.map((server) => (
            <li key={server.id} className="server-list-item">
              <div className="server-list-icon">
                <Icon name="server" size={16} />
              </div>
              <div className="server-list-main">
                <span className="server-list-name" title={server.name}>{server.name}</span>
                <HostAddress
                  value={`${server.host}:${server.sshPort}`}
                  prefix={server.username ? `${server.username}@` : undefined}
                  className="server-list-host"
                />
              </div>
              {canManage && localMatch(server) && (
                <Button
                  variant="ghost"
                  size="sm"
                  disabled={granting === server.id}
                  onClick={() => handleSync(server)}
                  title={t("teamServers.syncTitle")}
                >
                  <Icon name="key" size={14} />
                  {granting === server.id ? t("common.loading") : t("teamServers.sync")}
                </Button>
              )}
              {canManage && (
                <IconButton icon="trash" size="sm" danger title={t("teamServers.removeAria", { name: server.name })} onClick={() => handleDelete(server)} />
              )}
              {/* Said before the sync, not after: somebody looking at this
                  list needs to know this machine still has an account for a
                  person the team removed, whether or not they have pressed
                  anything today. Local match or not - if this install cannot
                  reach the Node, the warning matters more, not less. */}
              {pendingFor(server).length > 0 && (
                <ul className="team-servers-pending">
                  {pendingFor(server).map((revocation) => (
                    <li key={revocation.id}>
                      {t("teamServers.revocationPending", { email: revocation.email, account: revocation.nodeUsername })}
                    </li>
                  ))}
                  {!localMatch(server) && <li>{t("teamServers.revocationNoLocal")}</li>}
                </ul>
              )}

              {grantResults[server.id] && (
                <ul className="team-servers-grant-results">
                  {grantResults[server.id].members.map((result) => (
                    <li key={result.userId}>
                      {/* Per member, because four of five working is neither
                          a success nor a failure and the reader needs to know
                          which one did not. */}
                      {result.granted
                        ? t("teamServers.grantOk", { email: result.email, account: result.nodeUsername })
                        : result.hasKey
                          ? t("teamServers.grantFailed", { email: result.email, error: result.error ?? "" })
                          : t("teamServers.grantNoDevice", { email: result.email })}
                    </li>
                  ))}
                  {grantResults[server.id].revocations.map((revocation) => (
                    <li key={revocation.id}>
                      {revocation.completed
                        ? t("teamServers.revocationOk", { email: revocation.email, account: revocation.nodeUsername })
                        : t("teamServers.revocationFailed", { email: revocation.email, error: revocation.error ?? "" })}
                    </li>
                  ))}
                </ul>
              )}
            </li>
          ))}
        </ul>
      )}

      {canManage && (
        <form className="team-servers-form" onSubmit={handleCreate}>
          <div className="team-servers-form-row">
            <input className="form-input" placeholder={t("teamServers.namePlaceholder")} value={name} onChange={(e) => setName(e.target.value)} />
            <input className="form-input" placeholder={t("teamServers.hostPlaceholder")} value={host} onChange={(e) => setHost(e.target.value)} />
            <input
              className="form-input team-servers-port-input"
              placeholder={t("teamServers.portPlaceholder")}
              value={sshPort}
              onChange={(e) => setSshPort(e.target.value)}
              inputMode="numeric"
            />
            <input
              className="form-input"
              placeholder={t("teamServers.usernamePlaceholder")}
              value={username}
              onChange={(e) => setUsername(e.target.value)}
            />
            <Button type="submit" disabled={saving || !name.trim() || !host.trim()}>
              <Icon name="plus" size={14} />
              {saving ? t("common.loading") : t("teamServers.add")}
            </Button>
          </div>
          <p className="form-note">{t("teamServers.noSecretsNote")}</p>
        </form>
      )}
    </Card>
  );
}
