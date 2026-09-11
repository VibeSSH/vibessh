import { create } from "zustand";
import type { Application, ApplicationStatus } from "@/types/application";

interface ApplicationsState {
  applications: Application[];
  setApplications: (applications: Application[]) => void;
  upsertApplication: (application: Application) => void;
  updateStatus: (id: string, status: ApplicationStatus) => void;
  removeApplication: (id: string) => void;
}

export const useApplicationsStore = create<ApplicationsState>((set) => ({
  applications: [],
  setApplications: (applications) => set({ applications }),
  upsertApplication: (application) =>
    set((state) => {
      const existing = state.applications.findIndex((a) => a.id === application.id);
      if (existing === -1) return { applications: [...state.applications, application] };
      const applications = [...state.applications];
      applications[existing] = { ...applications[existing], ...application };
      return { applications };
    }),
  updateStatus: (id, status) =>
    set((state) => ({ applications: state.applications.map((a) => (a.id === id ? { ...a, status } : a)) })),
  removeApplication: (id) => set((state) => ({ applications: state.applications.filter((a) => a.id !== id) })),
}));
