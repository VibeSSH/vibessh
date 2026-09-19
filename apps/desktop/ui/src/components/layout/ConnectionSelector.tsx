import { useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { Icon } from "@/components/ui/Icon";
import { StatusDot } from "@/components/ui/StatusDot";
import { NodeIcon } from "@/components/servers/NodeIcon";
import { useServerModalStore } from "@/stores/serverModalStore";
import { useServersStore } from "@/stores/serversStore";
import { useSelectedNodeStore } from "@/stores/selectedNodeStore";
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
          <ul className="conn-selector-list">
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
    </div>
  );
}
