import { Outlet } from "react-router-dom";
import { ToastHost } from "@/components/ui/ToastHost";
import { Sidebar } from "./Sidebar";
import { Topbar } from "./Topbar";
import { TitleBar } from "./TitleBar";
import "./AppLayout.css";

export function AppLayout() {
  return (
    <div className="app-shell chrome-frame">
      <TitleBar />
      <div className="app-layout">
        <Sidebar />
        <div className="app-layout-main chrome-slab">
          <Topbar />
          <main className="app-layout-content">
            <Outlet />
          </main>
        </div>
      </div>
      <ToastHost />
    </div>
  );
}
