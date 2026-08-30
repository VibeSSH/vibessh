import { FormEvent, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { EmptyState } from "@/components/ui/EmptyState";
import { Icon } from "@/components/ui/Icon";
import { SkeletonRows } from "@/components/ui/SkeletonRows";
import { cloudCreateServer, cloudDeleteServer, cloudListServers } from "@/services/cloudService";
import { toastSuccess } from "@/stores/toastStore";
import type { CloudServer } from "@/types/cloud";
import "./ServersSection.css";

export function ServersSection({ teamId }: { teamId: string }) {
  const { t } = useTranslation();
  const [servers, setServers] = useState<CloudServer[]>([]);
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
      .catch((err) => setError(err instanceof Error ? err.message : t("teamServers.couldntList")))
      .finally(() => setLoading(false));
  }

  useEffect(load, [teamId]); // eslint-disable-line react-hooks/exhaustive-deps

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
      setError(err instanceof Error ? err.message : t("teamServers.couldntAdd"));
    } finally {
      setSaving(false);
    }
  }

  async function handleDelete(server: CloudServer) {
    setError(null);
    try {
      await cloudDeleteServer(teamId, server.id);
      toastSuccess(t("teamServers.removedToast", { name: server.name }));
      load();
    } catch (err) {
      setError(err instanceof Error ? err.message : t("teamServers.couldntRemove"));
    }
  }

  return (
    <Card title={t("teamServers.title")} subtitle={t("teamServers.subtitle")}>
      {error && <p className="page-error-note">{error}</p>}

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
                <span className="server-list-host" title={`${server.username ? `${server.username}@` : ""}${server.host}:${server.sshPort}`}>
                  {server.username ? `${server.username}@` : ""}
                  {server.host}:{server.sshPort}
                </span>
              </div>
              <button
                className="server-list-action"
                title={t("teamServers.removeAria", { name: server.name })}
                aria-label={t("teamServers.removeAria", { name: server.name })}
                onClick={() => handleDelete(server)}
              >
                <Icon name="trash" size={14} />
              </button>
            </li>
          ))}
        </ul>
      )}

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
    </Card>
  );
}
