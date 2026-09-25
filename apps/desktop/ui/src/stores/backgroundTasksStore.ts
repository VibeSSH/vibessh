import { create } from "zustand";

export type BackgroundTaskState = "running" | "done" | "failed";

/**
 * Work somebody stepped away from without cancelling.
 *
 * The first case was adding a server: closing the dialog mid-connect used to
 * throw the dialog away, so nobody could tell whether the Node had been added.
 * A dialog with work in flight now hides instead and leaves one of these on
 * the top bar; `open` brings the same dialog back, in the state it was left.
 */
export interface BackgroundTask {
  id: string;
  label: string;
  state: BackgroundTaskState;
  /** What went wrong, for a failed task - shown under its label. */
  detail?: string;
  open: () => void;
}

interface BackgroundTasksState {
  tasks: BackgroundTask[];
  /** Adds the task, or replaces the one with the same id. */
  upsert: (task: BackgroundTask) => void;
  remove: (id: string) => void;
}

export const useBackgroundTasksStore = create<BackgroundTasksState>((set) => ({
  tasks: [],
  upsert: (task) =>
    set((state) => {
      const index = state.tasks.findIndex((existing) => existing.id === task.id);
      if (index === -1) return { tasks: [...state.tasks, task] };
      const tasks = state.tasks.slice();
      tasks[index] = task;
      return { tasks };
    }),
  remove: (id) => set((state) => ({ tasks: state.tasks.filter((task) => task.id !== id) })),
}));
