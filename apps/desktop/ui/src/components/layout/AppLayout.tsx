import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { getAppInfo } from "@/services/appService";
import { Icon } from "@/components/ui/Icon";
import { Outlet } from "react-router-dom";
import { AuthModal } from "@/components/auth/AuthModal";
import { ToastHost } from "@/components/ui/ToastHost";
import { useBackupScheduler } from "@/hooks/useBackupScheduler";
import { startUpdateChecks } from "@/stores/updateStore";
import { UpdateBanner } from "./UpdateBanner";
import { useSmoothScroll } from "@/hooks/useSmoothScroll";
import { useTray } from "@/hooks/useTray";
import { cloudPublishThisDevice, cloudSessionInfo } from "@/services/cloudService";
import { useNodePermissionsStore } from "@/stores/nodePermissionsStore";
import { ForcePasswordChange } from "@/components/teams/ForcePasswordChange";
import { useAuthStore } from "@/stores/authStore";
import { GlobalServerModal } from "./GlobalServerModal";
import { SessionPasswordPrompt } from "@/components/servers/SessionPasswordPrompt";
import { Sidebar } from "./Sidebar";
import { TopBar } from "./TopBar";
import "./AppLayout.css";

export function AppLayout() {
  const scrollWrapperRef = useRef<HTMLElement | null>(null);
  const scrollContentRef = useRef<HTMLDivElement | null>(null);
  useSmoothScroll(scrollWrapperRef, scrollContentRef);

  const setUser = useAuthStore((s) => s.setUser);

  // One periodic check for the whole app, started where the shell is.
  useEffect(startUpdateChecks, []);
  // The tray menu's language, and its "Check for updates..." item, which
  // runs the check above rather than one of its own.
  useTray();

  const { t } = useTranslation();
  // Nothing in VibeSSH needs local root, but `sudo` is a natural thing to
  // reach for when something does not work - and it breaks the one thing
  // that fails least obviously: root has its own session and cannot see the
  // user's keyring, so every stored secret stops working with an error that
  // says nothing about root. Two people hit that before this warning existed.
  const [runningAsRoot, setRunningAsRoot] = useState(false);
  useEffect(() => {
    getAppInfo()
      .then((info) => setRunningAsRoot(info.runningAsRoot))
      .catch(() => undefined);
  }, []);
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
        // This machine's public key, republished on every restored session
        // and not only when somebody signs in.
        //
        // Publishing only from the sign-in modal meant an install that was
        // already signed in when this feature shipped never published at
        // all - and then a teammate syncing access was told that person
        // "has not opened VibeSSH on any device", while they had it open in
        // front of them. Idempotent by (user, key), so doing it on every
        // launch costs one request and cannot create a second device.
        cloudPublishThisDevice().catch((err) => console.warn("couldn't publish this device's key", err));
      } else {
        useNodePermissionsStore.getState().clear();
      }
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <div className="app-shell">
      {mustChangePassword && <ForcePasswordChange />}
      <TopBar />
      <div className="app-body">
        <Sidebar />
        {/* Lenis needs one element holding everything that scrolls, to watch
            it for resizes - a route renders whatever it likes, sometimes a
            fragment. This wrapper is that element. */}
        <main className="app-content" ref={scrollWrapperRef}>
          <div className="app-content-inner" ref={scrollContentRef}>
            {/* Not dismissable: it stays wrong for as long as the app is
                running as root, and the failures it explains happen later,
                when somebody tries to save a password. */}
            {runningAsRoot && (
              <p className="app-layout-root-warning">
                <Icon name="alert-triangle" size={16} />
                <span>{t("common.runningAsRoot")}</span>
              </p>
            )}
            <UpdateBanner />
            <Outlet />
          </div>
        </main>
      </div>
      <SessionPasswordPrompt />
      <ToastHost />
      <GlobalServerModal />
      <AuthModal />
    </div>
  );
}
