import { useEffect } from "react";
import { Outlet } from "react-router-dom";
import { AuthModal } from "@/components/auth/AuthModal";
import { ToastHost } from "@/components/ui/ToastHost";
import { cloudSessionInfo } from "@/services/cloudService";
import { useAuthStore } from "@/stores/authStore";
import { GlobalServerModal } from "./GlobalServerModal";
import { Sidebar } from "./Sidebar";
import { Rail } from "./Rail";
import { TitleBar } from "./TitleBar";
import "./AppLayout.css";

export function AppLayout() {
  const setUser = useAuthStore((s) => s.setUser);

  useEffect(() => {
    // A session from a previous launch may already be restored on the Rust
    // side by the time this resolves (see lib.rs's setup() spawning
    // cloud_try_restore_session) - this just asks what the current state
    // is, it doesn't do the restoring itself.
    cloudSessionInfo().then((info) => setUser(info?.user ?? null));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

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
      <AuthModal />
    </div>
  );
}
