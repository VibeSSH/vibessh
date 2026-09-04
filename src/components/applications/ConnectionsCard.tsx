import { useMemo, useState } from "react";
import { useQueries, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { EmptyState } from "@/components/ui/EmptyState";
import { Icon } from "@/components/ui/Icon";
import { Select } from "@/components/ui/Select";
import { IconButton } from "@/components/ui/IconButton";
import { SkeletonRows } from "@/components/ui/SkeletonRows";
import {
  connectApplications,
  disconnectApplications,
  listApplications,
  listApplicationLinks,
} from "@/services/applicationService";
import { queryKeys } from "@/services/queryKeys";
import { errorMessage } from "@/services/tauri";
import type { Application, ApplicationDetail } from "@/types/application";

interface ConnectionsCardProps {
  application: ApplicationDetail;
}

/** Which other Applications on this Node this one is allowed to reach.
 *
 * This exists because reachability between Applications used to be
 * unconditional and invisible: every container shared one Docker network, so
 * any Application could open a socket to any other Application's
 * *unpublished* ports - the database port deliberately left unpublished
 * included (`AUDIT_REPORT.md` S-018). Nothing in the UI said so, which is the
 * part that made it a boundary problem rather than a design choice.
 *
 * Now nothing is reachable until someone says so here, and this card is the
 * whole of that story: what is granted, and what could be. A connection is
 * symmetric because the Docker network implementing it is - the copy says so
 * rather than drawing an arrow the Node would not honour.
 *
 * Only Docker applications on the same Node can appear: a Docker network does
 * not span hosts, and a systemd unit or bare process is not on one at all. */
export function ConnectionsCard({ application }: ConnectionsCardProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [selected, setSelected] = useState("");
  const [busy, setBusy] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);

  // Two queries rather than one `Promise.all`: the list of applications is
  // the same answer every card on this page needs, so sharing its key means
  // it is fetched once and reused, while this card's own links stay keyed to
  // this application.
  const [linksQuery, applicationsQuery] = useQueries({
    queries: [
      { queryKey: queryKeys.applicationLinks(application.id), queryFn: () => listApplicationLinks(application.id) },
      { queryKey: queryKeys.applications(), queryFn: () => listApplications() },
    ],
  });

  const links = useMemo(() => linksQuery.data ?? [], [linksQuery.data]);
  const loading = linksQuery.isPending || applicationsQuery.isPending;
  const loadError = linksQuery.error ?? applicationsQuery.error;
  const error = actionError ?? (loadError ? errorMessage(loadError, t) : null);

  const candidates = useMemo(
    () =>
      (applicationsQuery.data ?? []).filter(
        (other: Application) =>
          other.id !== application.id &&
          other.runtimeType === "docker" &&
          other.serverId !== null &&
          other.serverId === application.serverId,
      ),
    [applicationsQuery.data, application.id, application.serverId],
  );

  async function reload() {
    await queryClient.invalidateQueries({ queryKey: queryKeys.applicationLinks(application.id) });
  }

  const byId = useMemo(() => new Map(candidates.map((app) => [app.id, app])), [candidates]);
  const connected = useMemo(() => links.map((id) => byId.get(id)).filter((app): app is Application => app !== undefined), [links, byId]);
  const available = useMemo(() => candidates.filter((app) => !links.includes(app.id)), [candidates, links]);

  async function run(action: () => Promise<void>) {
    setBusy(true);
    setActionError(null);
    try {
      await action();
      await reload();
      setSelected("");
    } catch (err) {
      setActionError(errorMessage(err, t));
    } finally {
      setBusy(false);
    }
  }

  // Only Docker applications have a private network to join, so for anything
  // else this card would be a permanently empty box. Saying nothing is
  // clearer than saying "none" about something that cannot exist.
  if (application.runtimeType !== "docker" || application.serverId === null) return null;

  return (
    <Card title={t("connections.title")}>
      <p className="form-note form-note-spaced">{t("connections.description")}</p>

      {error && <p className="form-note form-note-danger form-note-spaced">{error}</p>}

      {loading ? (
        <SkeletonRows />
      ) : connected.length === 0 ? (
        <EmptyState icon="wifi" title={t("connections.emptyTitle")} description={t("connections.emptyDescription")} />
      ) : (
        <ul className="server-list">
          {connected.map((peer) => (
            <li key={peer.id} className="server-list-item">
              <div className="server-list-main">
                <span className="server-list-name" title={peer.name}>
                  {peer.name}
                </span>
                <span className="server-list-host">{t("connections.reachableAs", { host: peer.name })}</span>
              </div>
              <IconButton
                icon="trash"
                size="sm"
                danger
                disabled={busy}
                title={t("connections.disconnectAria", { name: peer.name })}
                onClick={() => void run(() => disconnectApplications(application.id, peer.id))}
              />
            </li>
          ))}
        </ul>
      )}

      {available.length > 0 && (
        <div className="form-row">
          <label className="form-field form-field-grow">
            <span className="form-label">{t("connections.addLabel")}</span>
            <Select
              value={selected}
              onChange={setSelected}
              disabled={busy}
              placeholder={t("connections.addPlaceholder")}
              items={available.map((app) => ({ value: app.id, label: app.name }))}
            />
          </label>
          <div className="form-actions">
            <Button size="sm" disabled={busy || selected === ""} onClick={() => void run(() => connectApplications(application.id, selected))}>
              <Icon name="plus" size={14} />
              {busy ? t("common.saving") : t("connections.connect")}
            </Button>
          </div>
        </div>
      )}
    </Card>
  );
}
