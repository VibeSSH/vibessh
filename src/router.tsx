import { Navigate, Route, Routes } from "react-router-dom";
import { AppLayout } from "@/components/layout/AppLayout";
import { Dashboard } from "@/pages/Dashboard";
import { ActionsPage } from "@/pages/Actions";
import { FilesPage } from "@/pages/Files";
import { ModulePicker } from "@/pages/ModulePicker";
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
        <Route
          path="/terminal"
          element={<ModulePicker titleKey="modulePicker.terminalTitle" subtitleKey="modulePicker.terminalSubtitle" icon="terminal" routePrefix="/terminal" />}
        />
        <Route path="/terminal/:serverId" element={<TerminalPage />} />
        <Route
          path="/files"
          element={<ModulePicker titleKey="modulePicker.filesTitle" subtitleKey="modulePicker.filesSubtitle" icon="folder" routePrefix="/files" />}
        />
        <Route path="/files/:serverId" element={<FilesPage />} />
        <Route
          path="/monitor"
          element={<ModulePicker titleKey="modulePicker.monitorTitle" subtitleKey="modulePicker.monitorSubtitle" icon="activity" routePrefix="/monitor" />}
        />
        <Route path="/monitor/:serverId" element={<MonitorPage />} />
        <Route
          path="/actions"
          element={<ModulePicker titleKey="modulePicker.actionsTitle" subtitleKey="modulePicker.actionsSubtitle" icon="zap" routePrefix="/actions" />}
        />
        <Route path="/actions/:serverId" element={<ActionsPage />} />
        <Route path="/settings" element={<Settings />} />
        <Route path="*" element={<Navigate to="/" replace />} />
      </Route>
    </Routes>
  );
}
