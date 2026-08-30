import { useEffect, useState } from "react";
import { Card } from "@/components/ui/Card";
import { getAppInfo } from "@/services/appService";
import "./pages.css";

export function Settings() {
  const [appInfo, setAppInfo] = useState<{ name: string; version: string } | null>(null);

  useEffect(() => {
    getAppInfo()
      .then(setAppInfo)
      .catch(() => setAppInfo(null));
  }, []);

  return (
    <div className="page">
      <div className="page-header">
        <h1 className="page-title">Settings</h1>
        <p className="page-subtitle">Application preferences and diagnostics.</p>
      </div>

      <Card title="About" subtitle="Backend connectivity check">
        {appInfo ? (
          <p className="settings-row">
            {appInfo.name} <span className="settings-muted">v{appInfo.version}</span>
          </p>
        ) : (
          <p className="settings-muted">Waiting for Rust backend...</p>
        )}
      </Card>
    </div>
  );
}
