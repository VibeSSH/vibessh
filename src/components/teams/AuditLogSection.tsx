import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { EmptyState } from "@/components/ui/EmptyState";
import { SkeletonRows } from "@/components/ui/SkeletonRows";
import { cloudListAuditEvents } from "@/services/cloudService";
import type { CloudAuditEvent } from "@/types/cloud";
import "./AuditLogSection.css";

const PAGE_SIZE = 50;

interface AuditLogSectionProps {
  teamId: string;
}

export function AuditLogSection({ teamId }: AuditLogSectionProps) {
  const { t } = useTranslation();
  const [events, setEvents] = useState<CloudAuditEvent[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [hasMore, setHasMore] = useState(true);

  function loadPage(offset: number, replace: boolean) {
    (replace ? setLoading : setLoadingMore)(true);
    setError(null);
    cloudListAuditEvents(teamId, PAGE_SIZE, offset)
      .then((page) => {
        setEvents((prev) => (replace ? page : [...prev, ...page]));
        setHasMore(page.length === PAGE_SIZE);
      })
      .catch((err) => setError(err instanceof Error ? err.message : t("auditLog.couldntList")))
      .finally(() => (replace ? setLoading : setLoadingMore)(false));
  }

  useEffect(() => loadPage(0, true), [teamId]); // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <Card title={t("auditLog.title")} subtitle={t("auditLog.subtitle")}>
      {error && <p className="page-error-note">{error}</p>}
      {loading ? (
        <SkeletonRows count={6} />
      ) : events.length === 0 ? (
        <EmptyState icon="activity" title={t("auditLog.emptyTitle")} description={t("auditLog.emptyDescription")} />
      ) : (
        <>
          <ul className="audit-log-list">
            {events.map((event) => (
              <li key={event.id} className="audit-log-item">
                <div className="audit-log-item-main">
                  <span className="audit-log-action">{t(`auditLog.actions.${event.action}`, { defaultValue: event.action })}</span>
                  <span className="audit-log-actor">{event.actorDisplayName ?? event.actorEmail ?? t("auditLog.unknownActor")}</span>
                </div>
                <span className="audit-log-time">{new Date(event.createdAt).toLocaleString()}</span>
              </li>
            ))}
          </ul>
          {hasMore && (
            <div className="audit-log-load-more">
              <Button variant="secondary" size="sm" onClick={() => loadPage(events.length, false)} disabled={loadingMore}>
                {loadingMore ? t("common.loading") : t("auditLog.loadMore")}
              </Button>
            </div>
          )}
        </>
      )}
    </Card>
  );
}
