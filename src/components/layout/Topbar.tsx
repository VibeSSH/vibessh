import { useEffect, useState } from "react";
import { getAppInfo } from "@/services/appService";
import "./Topbar.css";

export function Topbar() {
  const [version, setVersion] = useState<string>("");

  useEffect(() => {
    getAppInfo()
      .then((info) => setVersion(info.version))
      .catch(() => setVersion(""));
  }, []);

  return (
    <header className="topbar">
      <div className="topbar-title">Server Manager</div>
      <div className="topbar-actions">
        {version && <span className="topbar-version">v{version}</span>}
      </div>
    </header>
  );
}
