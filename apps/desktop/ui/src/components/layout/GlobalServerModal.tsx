import { useCallback, useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { AddServerModal } from "@/components/servers/AddServerModal";
import { useBackgroundTasksStore } from "@/stores/backgroundTasksStore";
import { useServerModalStore } from "@/stores/serverModalStore";
import { toastError, useToastStore } from "@/stores/toastStore";

const TASK_ID = "server-modal";

/**
 * Mounted once in AppLayout so Rail's quick-add button (and Servers.tsx's own "Add server" button) can open the same modal from anywhere, controlled by one shared store instead of page-local state.
 *
 * Closing it while something is in flight - a connection, agent pairing, an
 * install in the setup wizard - hides it instead of unmounting it. Unmounting
 * used to throw the work's result away with the dialog, so nobody could tell
 * whether the server had been added. Hidden, it keeps running and keeps its
 * state, and a background task on the top bar brings it back as it was.
 */
export function GlobalServerModal() {
  const { t } = useTranslation();
  const isOpen = useServerModalStore((s) => s.isOpen);
  const editingServer = useServerModalStore((s) => s.editingServer);
  const minimized = useServerModalStore((s) => s.minimized);
  const activity = useServerModalStore((s) => s.activity);
  const close = useServerModalStore((s) => s.close);
  const minimize = useServerModalStore((s) => s.minimize);
  const restore = useServerModalStore((s) => s.restore);
  const reportActivity = useServerModalStore((s) => s.reportActivity);
  const upsertTask = useBackgroundTasksStore((s) => s.upsert);
  const removeTask = useBackgroundTasksStore((s) => s.remove);

  // Read from the store at call time, not from the render: a step can finish
  // and close the dialog in the same tick it reports it is no longer busy.
  const requestClose = useCallback(() => {
    if (useServerModalStore.getState().activity.busy) {
      minimize();
      useToastStore.getState().push(t("backgroundTasks.movedToBackground"), "info");
      return;
    }
    close();
  }, [close, minimize, t]);

  useEffect(() => {
    if (!isOpen || !minimized) {
      removeTask(TASK_ID);
      return;
    }
    upsertTask({
      id: TASK_ID,
      label: activity.label || t("backgroundTasks.addServer"),
      state: activity.busy ? "running" : activity.error ? "failed" : "done",
      detail: activity.error ?? undefined,
      open: restore,
    });
  }, [isOpen, minimized, activity, restore, upsertTask, removeTask, t]);

  // A failure behind a hidden dialog would otherwise sit unseen until
  // somebody thought to open the task. Said once per error, not per render.
  const announcedError = useRef<string | null>(null);
  useEffect(() => {
    if (!minimized || !activity.error) {
      if (!activity.error) announcedError.current = null;
      return;
    }
    if (announcedError.current === activity.error) return;
    announcedError.current = activity.error;
    toastError(activity.error);
  }, [minimized, activity.error]);

  if (!isOpen) return null;
  return (
    <div hidden={minimized}>
      <AddServerModal onClose={requestClose} editingServer={editingServer ?? undefined} onActivity={reportActivity} />
    </div>
  );
}
