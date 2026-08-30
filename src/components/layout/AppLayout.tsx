import { Outlet } from "react-router-dom";
import { ToastHost } from "@/components/ui/ToastHost";
import { Sidebar } from "./Sidebar";
import { Topbar } from "./Topbar";
import "./AppLayout.css";

export function AppLayout() {
  return (
    <div className="app-layout">
      <Sidebar />
      <div className="app-layout-main">
        <Topbar />
        <main className="app-layout-content">
          <Outlet />
        </main>
      </div>
      <ToastHost />
    </div>
  );
}
