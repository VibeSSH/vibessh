import { create } from "zustand";

/**
 * Which applications are mid-recreate in the background.
 *
 * A config change (environment, ports, image, limits) now persists instantly
 * and the container recreate that makes it take effect runs on its own without
 * blocking the form - see `useContainerApply`. This store is how the detail
 * header knows to show a quiet "applying changes" indicator while that
 * background recreate is in flight, instead of the save freezing on a generic
 * "Saving..." for the whole round trip.
 */
interface ApplicationApplyState {
  applying: Record<string, boolean>;
  start: (id: string) => void;
  finish: (id: string) => void;
}

export const useApplicationApplyStore = create<ApplicationApplyState>((set) => ({
  applying: {},
  start: (id) => set((state) => ({ applying: { ...state.applying, [id]: true } })),
  finish: (id) =>
    set((state) => {
      const next = { ...state.applying };
      delete next[id];
      return { applying: next };
    }),
}));

/** Whether the given application is currently applying a change to its container. */
export function useIsApplying(id: string | undefined): boolean {
  return useApplicationApplyStore((state) => (id ? Boolean(state.applying[id]) : false));
}
