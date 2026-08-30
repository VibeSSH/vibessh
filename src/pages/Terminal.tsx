import { useState } from "react";
import { Navigate, useNavigate, useParams } from "react-router-dom";
import { Button } from "@/components/ui/Button";
import { Icon } from "@/components/ui/Icon";
import { TerminalView } from "@/components/servers/TerminalView";
import { useServersStore } from "@/stores/serversStore";
import "./pages.css";
import "./Terminal.css";

export function TerminalPage() {
  const { serverId } = useParams<{ serverId: string }>();
  const navigate = useNavigate();
  const server = useServersStore((s) => s.servers.find((srv) => srv.id === serverId));
  const [closedReason, setClosedReason] = useState<string | null | undefined>(undefined);

  if (!serverId) {
    return <Navigate to="/servers" replace />;
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

      {closedReason !== undefined && (
        <p className="page-error-note">
          {closedReason ? `Session ended: ${closedReason}` : "Session ended."} Reopen this page to reconnect.
        </p>
      )}

      <div className="terminal-page-body">
        <TerminalView key={serverId} serverId={serverId} onClosed={setClosedReason} />
      </div>
    </div>
  );
}
