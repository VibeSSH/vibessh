import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { EmptyState } from "@/components/ui/EmptyState";
import { Icon } from "@/components/ui/Icon";
import "./pages.css";

export function Servers() {
  return (
    <div className="page">
      <div className="page-header page-header-row">
        <div>
          <h1 className="page-title">Servers</h1>
          <p className="page-subtitle">Manage the remote servers VibeSSH connects to.</p>
        </div>
        <Button disabled title="Wired up in Etap 2">
          <Icon name="plug" size={16} />
          Add server
        </Button>
      </div>

      <Card>
        <EmptyState
          icon="plug"
          title="No servers configured"
          description="Server storage, credentials, and connection testing are implemented in Etap 2 of the build. This page is the placeholder shell."
        />
      </Card>
    </div>
  );
}
