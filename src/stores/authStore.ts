import { create } from "zustand";
import type { CloudUserProfile } from "@/types/cloud";

interface AuthState {
  user: CloudUserProfile | null;
  /** "checking" only while the initial cloud_session_info() call (see AppLayout's mount effect) hasn't resolved yet - lets the Account popover avoid a one-frame "signed out" flash before a restored session loads. */
  status: "checking" | "signedOut" | "signedIn";
  setUser: (user: CloudUserProfile | null) => void;
}

export const useAuthStore = create<AuthState>((set) => ({
  user: null,
  status: "checking",
  setUser: (user) => set({ user, status: user ? "signedIn" : "signedOut" }),
}));
