import { useEffect, useState } from "react";
import { Navigate, useNavigate, useParams } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { Badge } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { Icon } from "@/components/ui/Icon";
import { SkeletonRows } from "@/components/ui/SkeletonRows";
import { cloudGetTeam, cloudListMembers } from "@/services/cloudService";
import type { CloudTeam, CloudTeamMember } from "@/types/cloud";
import "./pages.css";
import "./Servers.css";

export function TeamDetail() {
  const { t } = useTranslation();
  const { teamId } = useParams<{ teamId: string }>();
  const navigate = useNavigate();
  const [team, setTeam] = useState<CloudTeam | null>(null);
  const [members, setMembers] = useState<CloudTeamMember[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!teamId) return;
    setLoading(true);
    setError(null);
    Promise.all([cloudGetTeam(teamId), cloudListMembers(teamId)])
      .then(([loadedTeam, loadedMembers]) => {
        setTeam(loadedTeam);
        setMembers(loadedMembers);
      })
      .catch((err) => setError(err instanceof Error ? err.message : t("teams.couldntLoad")))
      .finally(() => setLoading(false));
  }, [teamId]); // eslint-disable-line react-hooks/exhaustive-deps

  if (!teamId) {
    return <Navigate to="/teams" replace />;
  }

  return (
    <div className="page">
      <div className="page-header page-header-row">
        <div>
          <h1 className="page-title">{team ? team.name : t("nav.teams")}</h1>
          <p className="page-subtitle">{t("teams.detailSubtitle")}</p>
        </div>
        <Button variant="secondary" onClick={() => navigate("/teams")}>
          <Icon name="chevron-left" size={16} />
          {t("teams.backToTeams")}
        </Button>
      </div>

      {error && <p className="page-error-note">{error}</p>}

      <Card title={t("teams.membersTitle")} subtitle={t("teams.membersCount", { count: members.length })}>
        {loading ? (
          <SkeletonRows />
        ) : (
          <ul className="server-list">
            {members.map((member) => (
              <li key={member.userId} className="server-list-item">
                <div className="server-list-icon">
                  <Icon name="user" size={16} />
                </div>
                <div className="server-list-main">
                  <span className="server-list-name">{member.displayName}</span>
                  <span className="server-list-host">{member.email}</span>
                </div>
                {member.isOwner && <Badge tone="success">{t("teams.owner")}</Badge>}
              </li>
            ))}
          </ul>
        )}
      </Card>
    </div>
  );
}
