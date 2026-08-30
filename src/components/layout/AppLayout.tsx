import { Outlet } from "react-router-dom";
import { ToastHost } from "@/components/ui/ToastHost";
import { GlobalServerModal } from "./GlobalServerModal";
import { Sidebar } from "./Sidebar";
import { Rail } from "./Rail";
import { TitleBar } from "./TitleBar";
import "./AppLayout.css";

export function AppLayout() {
  return (
    <div className="app-shell chrome-frame">
      <TitleBar />
      <div className="app-body">
        <Rail />
        <div className="app-layout-main chrome-slab">
          <div className="app-layout-brand">
            <img src="/vibessh-mark.svg" alt="" className="app-layout-brand-mark" />
            <span className="app-layout-brand-name">VibeSSH</span>
          </div>
          <div className="app-layout-row">
            <Sidebar />
            <main className="app-layout-content">
              <Outlet />
            </main>
          </div>
        </div>
      </div>
      <ToastHost />
      <GlobalServerModal />
    </div>
  );
}
