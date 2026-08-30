import { Card } from "@/components/ui/Card";
import { EmptyState } from "@/components/ui/EmptyState";
import "./pages.css";

export function Dashboard() {
  return (
    <div className="page">
      <div className="page-header">
        <h1 className="page-title">Dashboard</h1>
        <p className="page-subtitle">Overview of all your connected servers.</p>
      </div>

      <div className="stat-grid">
        <Card title="Servers">
          <div className="stat-value">0</div>
        </Card>
        <Card title="Online">
          <div className="stat-value stat-success">0</div>
        </Card>
        <Card title="Offline">
          <div className="stat-value stat-danger">0</div>
        </Card>
        <Card title="Alerts">
          <div className="stat-value">0</div>
        </Card>
      </div>

      <Card>
        <EmptyState
          icon="server"
          title="No servers yet"
          description="Add your first server to see its live status, resource usage, and quick actions here."
        />
      </Card>
    </div>
  );
}
