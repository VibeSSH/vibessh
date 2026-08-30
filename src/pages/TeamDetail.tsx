import { useEffect, useState } from "react";
import { Navigate, useNavigate, useParams } from "react-router-dom";
import { Trans, useTranslation } from "react-i18next";
import { Badge } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { Icon } from "@/components/ui/Icon";
import { IconButton } from "@/components/ui/IconButton";
import { SkeletonRows } from "@/components/ui/SkeletonRows";
import { AuditLogSection } from "@/components/teams/AuditLogSection";
import { InvitationsSection } from "@/components/teams/InvitationsSection";
import { MemberRolesEditor } from "@/components/teams/MemberRolesEditor";
import { RolesSection } from "@/components/teams/RolesSection";
import { ServersSection } from "@/components/teams/ServersSection";
import { AUDIT_VIEW, TEAM_DELETE, TEAM_MEMBERS_ADD, TEAM_MEMBERS_REMOVE, TEAM_ROLES_MANAGE, SERVERS_MANAGE } from "@/constants/permissions";
import { cloudDeleteTeam, cloudGetTeam, cloudListMembers, cloudMyPermissions, cloudRemoveMember } from "@/services/cloudService";
import { toastSuccess } from "@/stores/toastStore";
import type { CloudTeam, CloudTeamMember } from "@/types/cloud";
import "./pages.css";
import "./Servers.css";
import "./Teams.css";
import "@/components/servers/AddServerModal.css";
import "@/components/servers/forms.css";

type Tab = "members" | "roles" | "servers" | "invitations" | "audit";

export function TeamDetail() {
  const { t } = useTranslation();
  const { teamId } = useParams<{ teamId: string }>();
  const navigate = useNavigate();
  const [tab, setTab] = useState<Tab>("members");
  const [team, setTeam] = useState<CloudTeam | null>(null);
  const [members, setMembers] = useState<CloudTeamMember[]>([]);
  const [permissions, setPermissions] = useState<Set<string>>(new Set());
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [confirmingDelete, setConfirmingDelete] = useState(false);
  const [deleteBusy, setDeleteBusy] = useState(false);
  const [deleteError, setDeleteError] = useState<string | null>(null);

  function loadOverview() {
    if (!teamId) return;
    setLoading(true);
    setError(null);
    Promise.all([cloudGetTeam(teamId), cloudListMembers(teamId), cloudMyPermissions(teamId)])
      .then(([loadedTeam, loadedMembers, loadedPermissions]) => {
        setTeam(loadedTeam);
        setMembers(loadedMembers);
        setPermissions(new Set(loadedPermissions));
      })
      .catch((err) => setError(err instanceof Error ? err.message : t("teams.couldntLoad")))
      .finally(() => setLoading(false));
  }

  useEffect(loadOverview, [teamId]); // eslint-disable-line react-hooks/exhaustive-deps

  if (!teamId) {
    return <Navigate to="/teams" replace />;
  }

  const canManageRoles = permissions.has(TEAM_ROLES_MANAGE);
  const canManageServers = permissions.has(SERVERS_MANAGE);
  const canManageMembers = permissions.has(TEAM_MEMBERS_ADD);
  const canRemoveMembers = permissions.has(TEAM_MEMBERS_REMOVE);
  const canViewAudit = permissions.has(AUDIT_VIEW);
  const canDeleteTeam = permissions.has(TEAM_DELETE);

  async function handleRemoveMember(member: CloudTeamMember) {
    setError(null);
    try {
      await cloudRemoveMember(teamId!, member.userId);
      toastSuccess(t("teams.removedMemberToast", { name: member.displayName }));
      loadOverview();
    } catch (err) {
      setError(err instanceof Error ? err.message : t("teams.couldntRemoveMember"));
    }
  }

  async function handleConfirmDeleteTeam() {
    setDeleteBusy(true);
    setDeleteError(null);
    try {
      await cloudDeleteTeam(teamId!);
      toastSuccess(t("teams.deletedToast", { name: team?.name ?? "" }));
      navigate("/teams");
    } catch (err) {
      setDeleteError(err instanceof Error ? err.message : t("teams.couldntDeleteTeam"));
    } finally {
      setDeleteBusy(false);
    }
  }

  return (
    <div className="page">
      <div className="page-header page-header-row">
        <div>
          <h1 className="page-title">{team ? team.name : t("nav.teams")}</h1>
          <p className="page-subtitle">{t("teams.detailSubtitle")}</p>
        </div>
        <div className="team-detail-header-actions">
          {canDeleteTeam && (
            <Button variant="danger" size="sm" onClick={() => setConfirmingDelete(true)}>
              <Icon name="trash" size={14} />
              {t("teams.deleteTeam")}
            </Button>
          )}
          <Button variant="secondary" onClick={() => navigate("/teams")}>
            <Icon name="chevron-left" size={16} />
            {t("teams.backToTeams")}
          </Button>
        </div>
      </div>

      {error && <p className="page-error-note">{error}</p>}

      <div className="page-tabs">
        <button className={`modal-tab ${tab === "members" ? "modal-tab-active" : ""}`} onClick={() => setTab("members")}>
          {t("teams.membersTitle")}
        </button>
        <button className={`modal-tab ${tab === "roles" ? "modal-tab-active" : ""}`} onClick={() => setTab("roles")}>
          {t("roles.title")}
        </button>
        <button className={`modal-tab ${tab === "servers" ? "modal-tab-active" : ""}`} onClick={() => setTab("servers")}>
          {t("teamServers.title")}
        </button>
        {canManageMembers && (
          <button className={`modal-tab ${tab === "invitations" ? "modal-tab-active" : ""}`} onClick={() => setTab("invitations")}>
            {t("invitations.title")}
          </button>
        )}
        {canViewAudit && (
          <button className={`modal-tab ${tab === "audit" ? "modal-tab-active" : ""}`} onClick={() => setTab("audit")}>
            {t("auditLog.title")}
          </button>
        )}
      </div>

      {tab === "members" && (
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
                    <span className="server-list-name" title={member.displayName}>
                      {member.displayName}
                    </span>
                    <span className="server-list-host" title={member.email}>
                      {member.email}
                    </span>
                  </div>
                  {member.isOwner && <Badge tone="success">{t("teams.owner")}</Badge>}
                  <MemberRolesEditor
                    teamId={teamId}
                    userId={member.userId}
                    memberName={member.displayName}
                    isOwner={member.isOwner}
                    canManage={canManageRoles}
                  />
                  {canRemoveMembers && !member.isOwner && (
                    <IconButton
                      icon="trash"
                      size="sm"
                      danger
                      title={t("teams.removeMemberAria", { name: member.displayName })}
                      onClick={() => handleRemoveMember(member)}
                    />
                  )}
                </li>
              ))}
            </ul>
          )}
        </Card>
      )}

      {tab === "roles" && <RolesSection teamId={teamId} canManage={canManageRoles} />}
      {tab === "servers" && <ServersSection teamId={teamId} canManage={canManageServers} />}
      {tab === "invitations" && canManageMembers && <InvitationsSection teamId={teamId} canManage={canManageMembers} />}
      {tab === "audit" && canViewAudit && <AuditLogSection teamId={teamId} />}

      {confirmingDelete && (
        <div className="modal-backdrop" onClick={() => !deleteBusy && setConfirmingDelete(false)}>
          <div className="modal-panel modal-panel-sm" onClick={(e) => e.stopPropagation()}>
            <div className="modal-header">
              <h2 className="modal-title">{t("teams.deleteTeamTitle")}</h2>
              <IconButton icon="x" size="sm" onClick={() => setConfirmingDelete(false)} title={t("common.close")} />
            </div>
            <div className="modal-body">
              <p className="dialog-body-text">
                <Trans i18nKey="teams.deleteTeamBody" values={{ name: team?.name ?? "" }} components={{ 1: <strong /> }} />
              </p>
              {deleteError && <p className="form-note form-note-danger form-note-spaced">{deleteError}</p>}
              <div className="form-actions">
                <Button variant="secondary" onClick={() => setConfirmingDelete(false)} disabled={deleteBusy}>
                  {t("common.cancel")}
                </Button>
                <Button variant="danger" onClick={handleConfirmDeleteTeam} disabled={deleteBusy}>
                  {deleteBusy ? t("common.loading") : t("common.remove")}
                </Button>
              </div>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
