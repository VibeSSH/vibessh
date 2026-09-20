import { useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { Icon } from "@/components/ui/Icon";
import { StatusDot } from "@/components/ui/StatusDot";
import { NodeIcon } from "@/components/servers/NodeIcon";
import { useServerModalStore } from "@/stores/serverModalStore";
import { useServersStore } from "@/stores/serversStore";
import { useSelectedNodeStore } from "@/stores/selectedNodeStore";
import { useServerMetricsStore } from "@/stores/serverMetricsStore";
import { formatBytes } from "@/utils/formatBytes";
import "./ConnectionSelector.css";

/**
 * The context selector at the top of the sidebar. It names the Node the
 * dashboard is focused on and lets you switch it: picking a Node scopes the
 * dashboard panel (its metric tiles and workspace) to that Node rather than
 * opening the Node's console, and "all Nodes" returns to the fleet-wide
 * overview. The selection is shared with the dashboard's own Servers panel
 * through `useSelectedNodeStore`. The dropdown is also where a new server is
 * added.
 */
export function ConnectionSelector() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const servers = useServersStore((s) => s.servers);
  const openForCreate = useServerModalStore((s) => s.openForCreate);
  const selectedNodeId = useSelectedNodeStore((s) => s.selectedNodeId);
  const setSelectedNodeId = useSelectedNodeStore((s) => s.setSelectedNodeId);
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  // The hover card that restores the old rail preview: pointing at a Node in
  // the list shows its live disk/RAM/system/IP beside the menu. Readings come
  // from the shared metrics cache (whatever the Dashboard already fetched);
  // `ensure` fetches once if nothing fresh is cached, so a hover never opens a
  // connection of its own when the number is already known.
  const ensureMetrics = useServerMetricsStore((s) => s.ensure);
  const metricsByServer = useServerMetricsStore((s) => s.byServer);
  const [hovered, setHovered] = useState<{ id: string; top: number; left: number } | null>(null);

  useEffect(() => {
    if (!open) return;
    function handle(event: MouseEvent) {
      if (ref.current && !ref.current.contains(event.target as Node)) setOpen(false);
    }
    document.addEventListener("mousedown", handle);
    return () => document.removeEventListener("mousedown", handle);
  }, [open]);

  const activeServer = selectedNodeId ? servers.find((s) => s.id === selectedNodeId) : undefined;

  // Picking a Node (or "all Nodes") points the dashboard at it and takes you
  // there, so the panel data changes rather than a console opening.
  function focusNode(id: string | null) {
    setOpen(false);
    setSelectedNodeId(id);
    navigate("/");
  }

  return (
    <div className="conn-selector" ref={ref}>
      <button className="conn-selector-btn" onClick={() => setOpen((o) => !o)} aria-expanded={open} aria-haspopup="listbox">
        <span className="conn-selector-icon">
          {activeServer ? <NodeIcon server={activeServer} size={16} /> : <Icon name="server" size={16} />}
        </span>
        <span className="conn-selector-text">
          <span className="conn-selector-name">{activeServer ? activeServer.name : t("nav.servers")}</span>
          <span className="conn-selector-sub">
            {activeServer ? activeServer.host.replace(/./g, "•") : t("dashboard.clearSelection")}
          </span>
        </span>
        <Icon name="chevron-down" size={14} className={`conn-selector-caret ${open ? "conn-selector-caret-open" : ""}`} />
      </button>

      {open && (
        <div className="conn-selector-menu" role="listbox">
          <ul className="conn-selector-list" data-lenis-prevent>
            {/* The fleet-wide overview, marked active when no Node is picked. */}
            <li>
              <button
                className={`conn-selector-item ${!activeServer ? "conn-selector-item-active" : ""}`}
                onClick={() => focusNode(null)}
                role="option"
                aria-selected={!activeServer}
              >
                <Icon name="server" size={15} />
                <span className="conn-selector-item-name">{t("dashboard.clearSelection")}</span>
              </button>
            </li>
            {servers.map((server) => (
              <li key={server.id}>
                <button
                  className={`conn-selector-item ${activeServer?.id === server.id ? "conn-selector-item-active" : ""}`}
                  onClick={() => focusNode(server.id)}
                  role="option"
                  aria-selected={activeServer?.id === server.id}
                  onMouseEnter={(e) => {
                    const r = e.currentTarget.getBoundingClientRect();
                    setHovered({ id: server.id, top: r.top, left: r.right + 10 });
                    ensureMetrics(server.id);
                  }}
                  onMouseLeave={() => setHovered((h) => (h?.id === server.id ? null : h))}
                >
                  <NodeIcon server={server} size={15} />
                  <span className="conn-selector-item-name">{server.name}</span>
                  <StatusDot status={server.status} />
                </button>
              </li>
            ))}
          </ul>
          <button
            className="conn-selector-add"
            onClick={() => {
              setOpen(false);
              openForCreate();
            }}
          >
            <Icon name="plus" size={15} />
            {t("rail.addServer")}
          </button>
        </div>
      )}

      {open && hovered && (() => {
        const m = metricsByServer[hovered.id]?.metrics;
        const srv = servers.find((s) => s.id === hovered.id);
        return (
          <div className="conn-hover-card" style={{ top: hovered.top, left: hovered.left }} role="tooltip">
            <div className="conn-hover-name">{srv?.name}</div>
            <dl className="conn-hover-grid">
              <dt>IP</dt>
              <dd>{srv?.host ?? "—"}</dd>
              {m?.osName ? (
                <>
                  <dt>{t("rail.system")}</dt>
                  <dd>{m.osName}</dd>
                </>
              ) : null}
              <dt>{t("rail.ram")}</dt>
              <dd>{m ? `${formatBytes(m.ramUsedBytes)} / ${formatBytes(m.ramTotalBytes)}` : t("dashboard.nodeCollecting")}</dd>
              <dt>{t("rail.disk")}</dt>
              <dd>{m ? `${formatBytes(m.diskUsedBytes)} / ${formatBytes(m.diskTotalBytes)}` : "—"}</dd>
            </dl>
          </div>
        );
      })()}
    </div>
  );
}
