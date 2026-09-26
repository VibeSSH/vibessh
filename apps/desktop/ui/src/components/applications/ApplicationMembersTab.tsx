import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { Checkbox } from "@/components/ui/Checkbox";
import { EmptyState } from "@/components/ui/EmptyState";
import { Icon } from "@/components/ui/Icon";
import { SkeletonRows } from "@/components/ui/SkeletonRows";
import { Switch } from "@/components/ui/Switch";
import { APPLICATION_SCOPED, APPLICATIONS_CREATE, APPLICATIONS_FILES_READ, APPLICATIONS_FILES_WRITE } from "@/constants/permissions";
import {
  addApplicationMember,
  cloudListMembers,
  cloudListTeams,
  cloudMyPermissions,
  cloudSessionInfo,
  listApplicationMembers,
  listTeamApplications,
  removeApplicationMember,
  setApplicationMemberPermissions,
  shareApplicationWithTeam,
  type CloudApplication,
} from "@/services/cloudService";
import { CommandError, errorMessage } from "@/services/tauri";
import { toastError } from "@/stores/toastStore";
import type { CloudApplicationMember, CloudTeam, CloudTeamMember } from "@/types/cloud";
import "./ApplicationMembersTab.css";

/**
 * A bare "not found" with no backend error code - the member endpoints do not
 * exist on the backend this install is talking to yet.
 *
 * The backend names every real refusal (an application genuinely not shared
 * comes back as `application_not_shared`, carried in `params.backendCode`), so
 * a not-found with no code is not a refusal at all: it is a route that isn't
 * there, which is what an older or not-yet-updated backend returns. Telling the
 * two apart is what lets the tab say "the backend needs updating" instead of
 * showing a blank red "not found".
 */
function isEndpointMissing(error: unknown): boolean {
  return error instanceof CommandError && error.code === "not_found" && typeof error.params.backendCode !== "string";
}

/** Up to two letters for an avatar: first and last word of a name, or the start of an address. */
function initialsOf(name: string): string {
  const words = name.split(/[\s@._-]+/).filter(Boolean);
  if (words.length === 0) return "?";
  const first = words[0][0] ?? "";
  const last = words.length > 1 ? (words[words.length - 1][0] ?? "") : (words[0][1] ?? "");
  return `${first}${last}`.toUpperCase();
}

/**
 * Who, of a team, may see this application.
 *
 * This is the one screen where an application's visibility is decided per
 * person. Its meaning is opt-in and stated in the note above the list: an
 * application shared with a team but with nobody added here is visible to the
 * whole team, and adding the first person is what narrows it to exactly the
 * people listed. That is the behaviour the backend enforces.
 *
 * **It hides, it does not wall off.** Like every permission in VibeSSH this is
 * a guard rail inside the app, not a security boundary: somebody who can reach
 * the Node over SSH sees the application regardless. The note at the foot says
 * so, and it must not be dropped - it is the difference between "they can't
 * see it" and "they won't stumble into it", and only the second is true.
 */
export function ApplicationMembersTab({ applicationId }: { applicationId: string }) {
  const { t } = useTranslation();
  const navigate = useNavigate();

  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [signedIn, setSignedIn] = useState(false);
  const [teams, setTeams] = useState<CloudTeam[]>([]);
  const [teamId, setTeamId] = useState<string | null>(null);
  const [members, setMembers] = useState<CloudTeamMember[]>([]);
  const [canManage, setCanManage] = useState(false);
  /** What the signed-in person holds in this team - a permission they do not
   * hold cannot be handed on, and the backend refuses it, so its box is off. */
  const [myPermissions, setMyPermissions] = useState<string[]>([]);
  /** This application's projection in the selected team, or null when it has
   * not been shared there yet. Its `id` (not `applicationId`) is what the
   * member calls are keyed on. */
  const [sharedApp, setSharedApp] = useState<CloudApplication | null>(null);
  const [allowed, setAllowed] = useState<CloudApplicationMember[]>([]);
  /** The user id currently being toggled, so only its own switch shows busy. */
  const [busy, setBusy] = useState<string | null>(null);
  /** The backend does not have the per-application member endpoints yet.
   * Not an error - it becomes false the moment the updated backend ships. */
  const [pendingBackend, setPendingBackend] = useState(false);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      setLoading(true);
      try {
        const session = await cloudSessionInfo();
        if (cancelled) return;
        if (!session) {
          setSignedIn(false);
          return;
        }
        setSignedIn(true);
        const loadedTeams = await cloudListTeams();
        if (cancelled) return;
        setTeams(loadedTeams);
        setTeamId((previous) => previous ?? loadedTeams[0]?.id ?? null);
      } catch (err) {
        if (!cancelled) setError(errorMessage(err, t));
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
    // Runs once: the sign-in state and team list do not change while a tab is open.
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const loadTeam = useCallback(
    async (tid: string) => {
      setError(null);
      setPendingBackend(false);
      try {
        const [loadedMembers, permissions, shared] = await Promise.all([
          cloudListMembers(tid),
          cloudMyPermissions(tid),
          listTeamApplications(tid),
        ]);
        setMembers(loadedMembers);
        setCanManage(permissions.includes(APPLICATIONS_CREATE));
        setMyPermissions(permissions);
        let projection = shared.find((entry) => entry.localId === applicationId) ?? null;
        // Shared before the projection recorded its Node, which every share
        // did until now: pushed again, so the Rust side can find the team's
        // server for it. Without one, no permission set here can reach the
        // Node. Only by somebody who may share - anyone else just sees the
        // note below.
        if (projection && !projection.teamServerId && permissions.includes(APPLICATIONS_CREATE)) {
          try {
            projection = await shareApplicationWithTeam(tid, applicationId, null);
          } catch (err) {
            console.warn("couldn't record which Node this shared application is on", err);
          }
        }
        setSharedApp(projection);
        if (!projection) {
          setAllowed([]);
          return;
        }
        // The one call that hits a route the backend may not have yet. A bare
        // not-found here means "backend not updated", which is a calm state,
        // not the red error every other failure deserves.
        try {
          setAllowed(await listApplicationMembers(tid, projection.id));
        } catch (err) {
          if (isEndpointMissing(err)) {
            setPendingBackend(true);
            setAllowed([]);
          } else {
            throw err;
          }
        }
      } catch (err) {
        setError(errorMessage(err, t));
      }
    },
    [applicationId, t],
  );

  useEffect(() => {
    if (teamId) loadTeam(teamId);
  }, [teamId, loadTeam]);

  const allowedIds = useMemo(() => new Set(allowed.map((member) => member.userId)), [allowed]);
  const grantedPermissions = useMemo(() => new Map(allowed.map((member) => [member.userId, member.permissions ?? []])), [allowed]);

  /**
   * Ticks or unticks one permission for one member.
   *
   * Editing files without being able to read them is not a thing anybody
   * means, so the two move together: writing brings reading with it, and
   * taking reading away takes writing too.
   */
  async function togglePermission(member: CloudTeamMember, permission: string, next: boolean) {
    if (!teamId || !sharedApp) return;
    const current = new Set(grantedPermissions.get(member.userId) ?? []);
    if (next) {
      current.add(permission);
      if (permission === APPLICATIONS_FILES_WRITE) current.add(APPLICATIONS_FILES_READ);
    } else {
      current.delete(permission);
      if (permission === APPLICATIONS_FILES_READ) current.delete(APPLICATIONS_FILES_WRITE);
    }
    const permissions = APPLICATION_SCOPED.filter((key) => current.has(key));
    setBusy(member.userId);
    setError(null);
    try {
      await setApplicationMemberPermissions(teamId, sharedApp.id, member.userId, permissions);
      setAllowed((previous) => previous.map((entry) => (entry.userId === member.userId ? { ...entry, permissions } : entry)));
    } catch (err) {
      const message = errorMessage(err, t);
      setError(message);
      toastError(message);
    } finally {
      setBusy(null);
    }
  }

  async function toggle(member: CloudTeamMember, next: boolean) {
    if (!teamId) return;
    setBusy(member.userId);
    setError(null);
    try {
      let projection = sharedApp;
      // Adding the first person to an application nobody has shared yet shares
      // it as a side effect - the tab treats "let this person see it" as the
      // whole intent, and sharing is how that intent is carried out.
      if (next && !projection) {
        projection = await shareApplicationWithTeam(teamId, applicationId, null);
        setSharedApp(projection);
      }
      if (!projection) return;
      if (next) {
        await addApplicationMember(teamId, projection.id, member.userId);
      } else {
        await removeApplicationMember(teamId, projection.id, member.userId);
      }
      await loadTeam(teamId);
    } catch (err) {
      if (isEndpointMissing(err)) {
        setPendingBackend(true);
      } else {
        const message = errorMessage(err, t);
        setError(message);
        toastError(message);
      }
    } finally {
      setBusy(null);
    }
  }

  if (loading) {
    return (
      <Card title={t("applicationMembers.title")} subtitle={t("applicationMembers.subtitle")}>
        <SkeletonRows />
      </Card>
    );
  }

  if (!signedIn) {
    return (
      <Card title={t("applicationMembers.title")} subtitle={t("applicationMembers.subtitle")}>
        <EmptyState
          icon="users"
          title={t("applicationMembers.signedOutTitle")}
          description={t("applicationMembers.signedOutDescription")}
        />
      </Card>
    );
  }

  if (teams.length === 0) {
    return (
      <Card title={t("applicationMembers.title")} subtitle={t("applicationMembers.subtitle")}>
        <EmptyState
          icon="users"
          title={t("applicationMembers.noTeamsTitle")}
          description={t("applicationMembers.noTeamsDescription")}
          action={
            <Button variant="secondary" size="sm" onClick={() => navigate("/teams")}>
              <Icon name="chevron-right" size={14} />
              {t("applicationMembers.goToTeams")}
            </Button>
          }
        />
      </Card>
    );
  }

  const visibilityNote = !sharedApp
    ? t("applicationMembers.notSharedNote")
    : allowed.length === 0
      ? t("applicationMembers.teamWideNote")
      : t("applicationMembers.restrictedNote");

  return (
    <Card
      title={t("applicationMembers.title")}
      subtitle={t("applicationMembers.subtitle")}
      actions={
        <>
          {members.length > 0 && (
            <span className="application-members-count">
              {t("applicationMembers.accessCount", { granted: members.filter((member) => allowedIds.has(member.userId)).length, total: members.length })}
            </span>
          )}
          <Button variant="secondary" size="sm" onClick={() => navigate("/teams")}>
            <Icon name="users" size={14} />
            {t("applicationMembers.manageTeam")}
          </Button>
        </>
      }
    >
      {error && <p className="page-error-note">{error}</p>}

      {teams.length > 1 && (
        <div className="application-members-teams" role="tablist" aria-label={t("applicationMembers.teamSelectorAria")}>
          {teams.map((team) => (
            <button
              key={team.id}
              type="button"
              role="tab"
              aria-selected={team.id === teamId}
              className={`application-members-team${team.id === teamId ? " application-members-team-active" : ""}`}
              onClick={() => setTeamId(team.id)}
            >
              {team.name}
            </button>
          ))}
        </div>
      )}

      {pendingBackend ? (
        <p className="application-members-note application-members-note-pending">
          <Icon name="refresh-cw" size={14} />
          {t("applicationMembers.pendingBackendNote")}
        </p>
      ) : (
        <p className={`application-members-note application-members-note-${sharedApp ? (allowed.length === 0 ? "wide" : "restricted") : "unshared"}`}>
          <Icon name={sharedApp && allowed.length > 0 ? "lock" : "eye"} size={14} />
          {visibilityNote}
        </p>
      )}

      {!pendingBackend && !canManage && <p className="application-members-note">{t("applicationMembers.noPermissionNote")}</p>}

      {members.length === 0 ? (
        <EmptyState icon="users" title={t("applicationMembers.aloneTitle")} description={t("applicationMembers.aloneDescription")} />
      ) : (
        <ul className="application-members-list">
          {members.map((member) => {
            const granted = allowedIds.has(member.userId);
            return (
              <li key={member.userId} className="application-members-item">
                <span className="application-members-avatar" aria-hidden>
                  {initialsOf(member.displayName || member.email)}
                </span>
                <div className="application-members-main">
                  <span className="application-members-name">
                    {member.displayName || member.email}
                    {member.isOwner && <span className="application-members-owner-pill">{t("applicationMembers.ownerPill")}</span>}
                  </span>
                  <span className="application-members-email">{member.email}</span>
                </div>
                {/* Said in words beside the switch: it sits at the far edge
                    of a wide row, and a switch alone is easy to misread from
                    across the screen. */}
                <span className={`application-members-access${granted ? " application-members-access-granted" : ""}`}>
                  {granted ? t("applicationMembers.hasAccess") : t("applicationMembers.noAccess")}
                </span>
                <Switch
                  checked={granted}
                  disabled={!canManage || pendingBackend || busy === member.userId}
                  ariaLabel={t(granted ? "applicationMembers.revokeAria" : "applicationMembers.grantAria", {
                    name: member.displayName || member.email,
                  })}
                  onChange={(next) => toggle(member, next)}
                />
                {granted && (
                  <div className="application-members-permissions">
                    {member.isOwner ? (
                      <span className="application-members-permissions-owner">{t("applicationMembers.ownerHasAll")}</span>
                    ) : (
                      <>
                        <span className="application-members-permissions-label">{t("applicationMembers.canAlso")}</span>
                        {APPLICATION_SCOPED.map((permission) => (
                          <Checkbox
                            key={permission}
                            checked={(grantedPermissions.get(member.userId) ?? []).includes(permission)}
                            disabled={!canManage || busy === member.userId || !myPermissions.includes(permission)}
                            label={t(`roles.permissionLabels.${permission}`, { defaultValue: permission })}
                            onChange={(next) => void togglePermission(member, permission, next)}
                          />
                        ))}
                      </>
                    )}
                  </div>
                )}
              </li>
            );
          })}
        </ul>
      )}

      {sharedApp && !sharedApp.teamServerId && (
        <p className="application-members-note application-members-note-pending">
          <Icon name="alert-triangle" size={14} />
          {t("applicationMembers.noTeamServerNote")}
        </p>
      )}

      {sharedApp && sharedApp.teamServerId && allowed.length > 0 && (
        <p className="application-members-guardrail">
          <Icon name="refresh-cw" size={13} />
          {t("applicationMembers.permissionsSyncNote")}
        </p>
      )}

      <p className="application-members-guardrail">
        <Icon name="shield" size={13} />
        {t("applicationMembers.guardRailNote")}
      </p>
    </Card>
  );
}
