import { FormEvent, useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { EmptyState } from "@/components/ui/EmptyState";
import { Icon } from "@/components/ui/Icon";
import { SkeletonRows } from "@/components/ui/SkeletonRows";
import { cloudCreateTeam, cloudListTeams } from "@/services/cloudService";
import { toastSuccess } from "@/stores/toastStore";
import type { CloudTeam } from "@/types/cloud";
import "./pages.css";
import "./Servers.css";
import "./Files.css";
import "./Teams.css";

export function Teams() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [teams, setTeams] = useState<CloudTeam[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [newTeamName, setNewTeamName] = useState("");
  const [creating, setCreating] = useState(false);

  function load() {
    setLoading(true);
    setError(null);
    cloudListTeams()
      .then(setTeams)
      .catch((err) => setError(err instanceof Error ? err.message : t("teams.couldntList")))
      .finally(() => setLoading(false));
  }

  useEffect(load, []); // eslint-disable-line react-hooks/exhaustive-deps

  async function handleCreate(e: FormEvent) {
    e.preventDefault();
    const name = newTeamName.trim();
    if (!name) return;
    setCreating(true);
    setError(null);
    try {
      const team = await cloudCreateTeam(name);
      setTeams((prev) => [...prev, team]);
      setNewTeamName("");
      toastSuccess(t("teams.createdToast", { name: team.name }));
    } catch (err) {
      setError(err instanceof Error ? err.message : t("teams.couldntCreate"));
    } finally {
      setCreating(false);
    }
  }

  return (
    <div className="page">
      <div className="page-header">
        <h1 className="page-title">{t("teams.title")}</h1>
        <p className="page-subtitle">{t("teams.subtitle")}</p>
      </div>

      <Card title={t("teams.createTitle")}>
        <form className="teams-create-row" onSubmit={handleCreate}>
          <input
            className="form-input"
            placeholder={t("teams.namePlaceholder")}
            value={newTeamName}
            onChange={(e) => setNewTeamName(e.target.value)}
          />
          <Button type="submit" disabled={creating || !newTeamName.trim()}>
            <Icon name="plus" size={14} />
            {creating ? t("common.loading") : t("teams.create")}
          </Button>
        </form>
      </Card>

      {error && <p className="page-error-note">{error}</p>}

      <Card>
        {loading ? (
          <SkeletonRows />
        ) : teams.length === 0 ? (
          <EmptyState icon="users" title={t("teams.emptyTitle")} description={t("teams.emptyDescription")} />
        ) : (
          <ul className="server-list">
            {teams.map((team) => (
              <li key={team.id} className="server-list-item">
                <div className="server-list-icon">
                  <Icon name="users" size={16} />
                </div>
                <button className="files-entry-name" title={team.name} onClick={() => navigate(`/teams/${team.id}`)}>
                  {team.name}
                </button>
              </li>
            ))}
          </ul>
        )}
      </Card>
    </div>
  );
}
