import { FormEvent, useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { EmptyState } from "@/components/ui/EmptyState";
import { Icon } from "@/components/ui/Icon";
import { SkeletonRows } from "@/components/ui/SkeletonRows";
import { cloudAcceptInvitation, cloudCreateTeam, cloudDeclineInvitation, cloudListTeams } from "@/services/cloudService";
import { toastSuccess } from "@/stores/toastStore";
import type { CloudTeam } from "@/types/cloud";
import "./pages.css";
import "./Servers.css";
import "./Files.css";
import "./Teams.css";
import "@/components/servers/forms.css";
import { errorMessage } from "@/services/tauri";

export function Teams() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [teams, setTeams] = useState<CloudTeam[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [newTeamName, setNewTeamName] = useState("");
  const [creating, setCreating] = useState(false);
  const [invitationToken, setInvitationToken] = useState("");
  const [invitationBusy, setInvitationBusy] = useState(false);
  const [invitationError, setInvitationError] = useState<string | null>(null);

  function load() {
    setLoading(true);
    setError(null);
    cloudListTeams()
      .then(setTeams)
      .catch((err) => setError(errorMessage(err, t)))
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
      setError(errorMessage(err, t));
    } finally {
      setCreating(false);
    }
  }

  async function handleAcceptInvitation() {
    const token = invitationToken.trim();
    if (!token) return;
    setInvitationBusy(true);
    setInvitationError(null);
    try {
      const team = await cloudAcceptInvitation(token);
      setInvitationToken("");
      toastSuccess(t("teams.acceptedInvitationToast", { name: team.name }));
      load();
    } catch (err) {
      setInvitationError(errorMessage(err, t));
    } finally {
      setInvitationBusy(false);
    }
  }

  async function handleDeclineInvitation() {
    const token = invitationToken.trim();
    if (!token) return;
    setInvitationBusy(true);
    setInvitationError(null);
    try {
      await cloudDeclineInvitation(token);
      setInvitationToken("");
      toastSuccess(t("teams.declinedInvitationToast"));
    } catch (err) {
      setInvitationError(errorMessage(err, t));
    } finally {
      setInvitationBusy(false);
    }
  }

  return (
    <div className="page">
      <div className="page-header">
        <h1 className="page-title">{t("teams.title")}</h1>
        <p className="page-subtitle">{t("teams.subtitle")}</p>
      </div>

      <Card title={t("teams.acceptInvitationTitle")} subtitle={t("teams.acceptInvitationSubtitle")}>
        <div className="teams-accept-row">
          <input
            className="form-input"
            placeholder={t("teams.invitationTokenPlaceholder")}
            value={invitationToken}
            onChange={(e) => setInvitationToken(e.target.value)}
          />
          <Button type="button" variant="secondary" onClick={handleDeclineInvitation} disabled={invitationBusy || !invitationToken.trim()}>
            {t("teams.decline")}
          </Button>
          <Button type="button" onClick={handleAcceptInvitation} disabled={invitationBusy || !invitationToken.trim()}>
            {invitationBusy ? t("common.loading") : t("teams.accept")}
          </Button>
        </div>
        {invitationError && <p className="form-note form-note-danger form-note-spaced">{invitationError}</p>}
      </Card>

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
