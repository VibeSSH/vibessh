import { useState } from "react";
import { Navigate, useNavigate, useParams } from "react-router-dom";
import { Button } from "@/components/ui/Button";
import { Icon } from "@/components/ui/Icon";
import { TerminalView } from "@/components/servers/TerminalView";
import { useServersStore } from "@/stores/serversStore";
import "./pages.css";
import "./Terminal.css";

interface TerminalTab {
  id: string;
  label: string;
}

function makeTab(number: number): TerminalTab {
  return { id: crypto.randomUUID(), label: `Shell ${number}` };
}

/**
 * Every tab is its own independent backend terminal session (SshSession
 * doesn't limit how many channels one connection can open) - closing a tab
 * closes only that shell, the others keep running. All tabs stay mounted at
 * once, shown/hidden with a plain CSS display toggle rather than conditional
 * rendering, so a backgrounded shell keeps receiving output instead of
 * disconnecting and losing scrollback every time you switch away from it.
 */
export function TerminalPage() {
  const { serverId } = useParams<{ serverId: string }>();
  const navigate = useNavigate();
  const server = useServersStore((s) => s.servers.find((srv) => srv.id === serverId));

  const [tabs, setTabs] = useState<TerminalTab[]>(() => [makeTab(1)]);
  const [activeTabId, setActiveTabId] = useState(() => tabs[0].id);
  const [nextTabNumber, setNextTabNumber] = useState(2);
  const [closedReasons, setClosedReasons] = useState<Record<string, string | null>>({});

  if (!serverId) {
    return <Navigate to="/servers" replace />;
  }

  function addTab() {
    const tab = makeTab(nextTabNumber);
    setNextTabNumber((n) => n + 1);
    setTabs((prev) => [...prev, tab]);
    setActiveTabId(tab.id);
  }

  function closeTab(id: string) {
    setTabs((prev) => {
      const next = prev.filter((tab) => tab.id !== id);
      if (activeTabId === id && next.length > 0) {
        setActiveTabId(next[next.length - 1].id);
      }
      return next;
    });
    setClosedReasons((prev) => {
      const rest = { ...prev };
      delete rest[id];
      return rest;
    });
  }

  return (
    <div className="page terminal-page">
      <div className="page-header page-header-row">
        <div>
          <h1 className="page-title">{server ? server.name : "Terminal"}</h1>
          <p className="page-subtitle">{server ? server.host : serverId}</p>
        </div>
        <Button variant="secondary" onClick={() => navigate("/servers")}>
          <Icon name="chevron-left" size={16} />
          Back to servers
        </Button>
      </div>

      <div className="terminal-tabs">
        {tabs.map((tab) => (
          <button
            key={tab.id}
            className={`terminal-tab${tab.id === activeTabId ? " terminal-tab-active" : ""}`}
            onClick={() => setActiveTabId(tab.id)}
          >
            <span>{tab.label}</span>
            <span
              role="button"
              tabIndex={0}
              aria-label={`Close ${tab.label}`}
              className="terminal-tab-close"
              onClick={(e) => {
                e.stopPropagation();
                closeTab(tab.id);
              }}
            >
              <Icon name="x" size={12} />
            </span>
          </button>
        ))}
        <button className="terminal-tab terminal-tab-add" aria-label="New terminal tab" onClick={addTab}>
          <Icon name="plus" size={14} />
        </button>
      </div>

      {tabs.map(
        (tab) =>
          closedReasons[tab.id] !== undefined && (
            <p key={tab.id} className="page-error-note" style={{ display: tab.id === activeTabId ? "block" : "none" }}>
              {closedReasons[tab.id] ? `Session ended: ${closedReasons[tab.id]}` : "Session ended."} Close this tab and
              open a new one to reconnect.
            </p>
          ),
      )}

      <div className="terminal-page-body">
        {tabs.length === 0 ? (
          <div className="terminal-empty-state">
            <p>No terminal sessions open.</p>
            <Button onClick={addTab}>
              <Icon name="plus" size={14} />
              New terminal
            </Button>
          </div>
        ) : (
          tabs.map((tab) => (
            <div key={tab.id} className="terminal-tab-pane" style={{ display: tab.id === activeTabId ? "block" : "none" }}>
              <TerminalView
                serverId={serverId}
                onClosed={(reason) => setClosedReasons((prev) => ({ ...prev, [tab.id]: reason }))}
              />
            </div>
          ))
        )}
      </div>
    </div>
  );
}
