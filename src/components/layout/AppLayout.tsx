import { Outlet } from "react-router-dom";
import { ToastHost } from "@/components/ui/ToastHost";
import { NavBar } from "./NavBar";
import { TitleBar } from "./TitleBar";
import "./AppLayout.css";

export function AppLayout() {
  return (
    <div className="app-shell chrome-frame">
      <TitleBar />
      <div className="app-layout-main chrome-slab">
        <NavBar />
        <main className="app-layout-content">
          <Outlet />
        </main>
      </div>
      <ToastHost />
    </div>
  );
}
