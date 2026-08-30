import { Navigate, Route, Routes } from "react-router-dom";
import { AppLayout } from "@/components/layout/AppLayout";
import { Dashboard } from "@/pages/Dashboard";
import { FilesPage } from "@/pages/Files";
import { MonitorPage } from "@/pages/Monitor";
import { Servers } from "@/pages/Servers";
import { Settings } from "@/pages/Settings";
import { TerminalPage } from "@/pages/Terminal";

export function AppRouter() {
  return (
    <Routes>
      <Route element={<AppLayout />}>
        <Route path="/" element={<Dashboard />} />
        <Route path="/servers" element={<Servers />} />
        <Route path="/terminal/:serverId" element={<TerminalPage />} />
        <Route path="/files/:serverId" element={<FilesPage />} />
        <Route path="/monitor/:serverId" element={<MonitorPage />} />
        <Route path="/settings" element={<Settings />} />
        <Route path="*" element={<Navigate to="/" replace />} />
      </Route>
    </Routes>
  );
}
