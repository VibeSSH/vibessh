import { useEffect, useRef } from "react";
import { Outlet } from "react-router-dom";
import { AuthModal } from "@/components/auth/AuthModal";
import { ToastHost } from "@/components/ui/ToastHost";
import { useBackupScheduler } from "@/hooks/useBackupScheduler";
import { useSmoothScroll } from "@/hooks/useSmoothScroll";
import { cloudSessionInfo } from "@/services/cloudService";
import { useNodePermissionsStore } from "@/stores/nodePermissionsStore";
import { ForcePasswordChange } from "@/components/teams/ForcePasswordChange";
import { useAuthStore } from "@/stores/authStore";
import { GlobalServerModal } from "./GlobalServerModal";
import { Sidebar } from "./Sidebar";
import { Rail } from "./Rail";
import { TitleBar } from "./TitleBar";
import "./AppLayout.css";

export function AppLayout() {
  const scrollWrapperRef = useRef<HTMLElement | null>(null);
  const scrollContentRef = useRef<HTMLDivElement | null>(null);
  useSmoothScroll(scrollWrapperRef, scrollContentRef);

  const setUser = useAuthStore((s) => s.setUser);
  // An account holding a password somebody else set can do nothing until
  // it replaces it - the backend refuses every other request - so the
  // whole interface is covered rather than letting them wander into it.
  const mustChangePassword = useAuthStore((s) => s.user?.mustChangePassword ?? false);
  useBackupScheduler();

  useEffect(() => {
    // A session from a previous launch may already be restored on the Rust
    // side by the time this resolves (see lib.rs's setup() spawning
    // cloud_try_restore_session) - this just asks what the current state
    // is, it doesn't do the restoring itself.
    cloudSessionInfo().then((info) => {
      setUser(info?.user ?? null);
      // Team guard rails, loaded once a session is known to exist. Signed
      // out there is nothing to load and nothing is restricted - see
      // `nodePermissionsStore` for why "not loaded" means "permitted".
      if (info?.user) {
        void useNodePermissionsStore.getState().load();
      } else {
        useNodePermissionsStore.getState().clear();
      }
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <div className="app-shell chrome-frame">
      {mustChangePassword && <ForcePasswordChange />}
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
            {/* Lenis needs one element holding everything that scrolls, to
                watch it for resizes - a route renders whatever it likes,
                sometimes a fragment. This wrapper is that element. */}
            <main className="app-layout-content" ref={scrollWrapperRef}>
              <div className="app-layout-scroll-content" ref={scrollContentRef}>
                <Outlet />
              </div>
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
