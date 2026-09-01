import { Navigate, Route, Routes } from "react-router-dom";
import { AppLayout } from "@/components/layout/AppLayout";
import { Dashboard } from "@/pages/Dashboard";
import { ActionsPage } from "@/pages/Actions";
import { Applications } from "@/pages/Applications";
import { ApplicationDetail } from "@/pages/ApplicationDetail";
import { DatabaseHosts } from "@/pages/DatabaseHosts";
import { FilesPage } from "@/pages/Files";
import { FirewallPage } from "@/pages/Firewall";
import { ModulePicker } from "@/pages/ModulePicker";
import { MonitorPage } from "@/pages/Monitor";
import { PortForwardingPage } from "@/pages/PortForwarding";
import { Servers } from "@/pages/Servers";
import { Settings } from "@/pages/Settings";
import { TeamDetail } from "@/pages/TeamDetail";
import { Teams } from "@/pages/Teams";
import { TerminalPage } from "@/pages/Terminal";
import { VibeNetwork } from "@/pages/VibeNetwork";

export function AppRouter() {
  return (
    <Routes>
      <Route element={<AppLayout />}>
        <Route path="/" element={<Dashboard />} />
        <Route path="/servers" element={<Servers />} />
        <Route path="/applications" element={<Applications />} />
        <Route path="/applications/:id" element={<ApplicationDetail />} />
        <Route path="/database-hosts" element={<DatabaseHosts />} />
        <Route path="/vibe-network" element={<VibeNetwork />} />
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
        <Route
          path="/port-forwarding"
          element={
            <ModulePicker
              titleKey="modulePicker.portForwardingTitle"
              subtitleKey="modulePicker.portForwardingSubtitle"
              icon="arrow-left-right"
              routePrefix="/port-forwarding"
            />
          }
        />
        <Route path="/port-forwarding/:serverId" element={<PortForwardingPage />} />
        <Route
          path="/firewall"
          element={<ModulePicker titleKey="modulePicker.firewallTitle" subtitleKey="modulePicker.firewallSubtitle" icon="lock" routePrefix="/firewall" />}
        />
        <Route path="/firewall/:serverId" element={<FirewallPage />} />
        <Route path="/teams" element={<Teams />} />
        <Route path="/teams/:teamId" element={<TeamDetail />} />
        <Route path="/settings" element={<Settings />} />
        <Route path="*" element={<Navigate to="/" replace />} />
      </Route>
    </Routes>
  );
}
