import { useEffect, useRef, useState } from "react";
import { useLocation, useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { Icon } from "@/components/ui/Icon";
import { StatusDot } from "@/components/ui/StatusDot";
import { NodeIcon } from "@/components/servers/NodeIcon";
import { useServerModalStore } from "@/stores/serverModalStore";
import { useServersStore } from "@/stores/serversStore";
import "./ConnectionSelector.css";

/**
 * The context selector at the top of the sidebar. It carries what the far-left
 * rail used to: it names the connection the current view is scoped to (a
 * per-server route like /terminal/:id or /monitor/:id), or "all servers" when
 * the view is global, and its dropdown is the quick server switcher and the
 * add-server action. No new capability - the rail's behaviour in one control.
 */
const SERVER_ROUTE = /^\/(?:terminal|files|monitor|actions|port-forwarding|firewall)\/([^/]+)/;

export function ConnectionSelector() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { pathname } = useLocation();
  const servers = useServersStore((s) => s.servers);
  const openForCreate = useServerModalStore((s) => s.openForCreate);
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

  const match = pathname.match(SERVER_ROUTE);
  const activeServer = match ? servers.find((s) => s.id === match[1]) : undefined;

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
          {servers.length === 0 ? (
            <p className="conn-selector-empty">{t("servers.emptyTitle")}</p>
          ) : (
            <ul className="conn-selector-list">
              {servers.map((server) => (
                <li key={server.id}>
                  <button
                    className={`conn-selector-item ${activeServer?.id === server.id ? "conn-selector-item-active" : ""}`}
                    onClick={() => {
                      setOpen(false);
                      navigate(server.connectionMode === "agent" ? "/servers" : `/terminal/${server.id}`);
                    }}
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
          )}
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
