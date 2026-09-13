import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { EmptyState } from "@/components/ui/EmptyState";
import { Icon } from "@/components/ui/Icon";
import { IconButton } from "@/components/ui/IconButton";
import { SkeletonRows } from "@/components/ui/SkeletonRows";
import {
  listTeamApplications,
  shareApplicationWithTeam,
  unshareApplicationFromTeam,
  type CloudApplication,
} from "@/services/cloudService";
import { listApplications } from "@/services/applicationService";
import { errorMessage } from "@/services/tauri";
import { toastSuccess } from "@/stores/toastStore";
import type { Application } from "@/types/application";
import "./ApplicationsSection.css";

/**
 * The Applications a team can see.
 *
 * Each one is a snapshot pushed by the install that owns it, not the record
 * that install acts on - so what is shown here can be out of date, and the
 * only honest way to say so is to show when it was last refreshed.
 */
export function ApplicationsSection({ teamId, canManage }: { teamId: string; canManage: boolean }) {
  const { t } = useTranslation();
  const [shared, setShared] = useState<CloudApplication[]>([]);
  const [local, setLocal] = useState<Application[]>([]);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  function load() {
    setLoading(true);
    listTeamApplications(teamId)
      .then(setShared)
      .catch((err) => setError(errorMessage(err, t)))
      .finally(() => setLoading(false));
  }

  useEffect(load, [teamId]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    listApplications()
      .then(setLocal)
      .catch(() => setLocal([]));
  }, []);

  /** This install's Applications that are not already shared. */
  const shareable = local.filter((application) => !shared.some((entry) => entry.localId === application.id));

  async function handleShare(application: Application) {
    setBusy(application.id);
    setError(null);
    try {
      await shareApplicationWithTeam(teamId, application.id, null);
      toastSuccess(t("teamApplications.sharedToast", { name: application.name }));
      load();
    } catch (err) {
      setError(errorMessage(err, t));
    } finally {
      setBusy(null);
    }
  }

  async function handleUnshare(application: CloudApplication) {
    setBusy(application.id);
    setError(null);
    try {
      await unshareApplicationFromTeam(teamId, application.id);
      toastSuccess(t("teamApplications.unsharedToast", { name: application.name }));
      load();
    } catch (err) {
      setError(errorMessage(err, t));
    } finally {
      setBusy(null);
    }
  }

  return (
    <Card title={t("teamApplications.title")} subtitle={t("teamApplications.subtitle")}>
      {error && <p className="page-error-note">{error}</p>}

      {loading ? (
        <SkeletonRows />
      ) : shared.length === 0 ? (
        <EmptyState icon="box" title={t("teamApplications.emptyTitle")} description={t("teamApplications.emptyDescription")} />
      ) : (
        <ul className="team-applications-list">
          {shared.map((application) => (
            <li key={application.id} className="team-applications-item">
              <div className="team-applications-main">
                <span className="team-applications-name">{application.name}</span>
                <span className="team-applications-meta">
                  {/* When the snapshot was taken, not when the Application
                      changed. Without it there is no way to tell a current
                      projection from one pushed three weeks ago. */}
                  {t("teamApplications.updatedAt", { when: new Date(application.updatedAt).toLocaleString() })}
                </span>
                {application.environment.some((variable) => variable.isSecret) && (
                  <span className="team-applications-meta">{t("teamApplications.secretsHidden")}</span>
                )}
              </div>
              {canManage && (
                <IconButton
                  icon="trash"
                  size="sm"
                  danger
                  disabled={busy === application.id}
                  title={t("teamApplications.unshareAria", { name: application.name })}
                  onClick={() => handleUnshare(application)}
                />
              )}
            </li>
          ))}
        </ul>
      )}

      {canManage && shareable.length > 0 && (
        <div className="team-applications-share">
          <p className="team-applications-meta">{t("teamApplications.shareHint")}</p>
          <ul className="team-applications-list">
            {shareable.map((application) => (
              <li key={application.id} className="team-applications-item">
                <span className="team-applications-name">{application.name}</span>
                <Button variant="ghost" size="sm" disabled={busy === application.id} onClick={() => handleShare(application)}>
                  <Icon name="plus" size={14} />
                  {busy === application.id ? t("common.loading") : t("teamApplications.share")}
                </Button>
              </li>
            ))}
          </ul>
        </div>
      )}
    </Card>
  );
}
