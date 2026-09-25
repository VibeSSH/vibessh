import { useQuery } from "@tanstack/react-query";
import { POLL_INTERVALS } from "@/hooks/usePolling";
import { readApplicationFile } from "@/services/applicationFilesService";
import { bytesToText } from "@/services/filesService";

/** Mirrors the VibeSSH Scheduler plugin's `scheduler.json` - see the plugin's `ScheduleStatus`. */
export interface SchedulerStatus {
  schema: number;
  updatedAt: string;
  /** ISO instant of the next restart, or null when nothing is scheduled. */
  nextRestart: string | null;
  /** Seconds until the next restart at the moment the file was written, or -1 when none. */
  secondsUntil: number;
  reason: string | null;
  countingDown: boolean;
  method: string;
}

/** Where the VibeSSH Scheduler plugin writes, relative to the application's working directory. */
const STATUS_PATH = ".vibessh/scheduler.json";
/** How often to look again for the plugin on a server that did not have it last time. */
const ABSENT_RECHECK_MS = 60_000;

/**
 * The live restart schedule of an application, or null when it is not running the VibeSSH
 * Scheduler plugin.
 *
 * Drives both whether the Restarts tab is shown at all and what it renders, so the tab appears
 * exactly for the servers that can fill it. Reads `scheduler.json` over the same SSH connection
 * the rest of the page uses, on the detail poll; a missing file is "no scheduler here", not an
 * error. {@link fetchedAt} is when this reading arrived on the client, which the card uses to
 * tick the countdown down every second between polls without trusting the two clocks to agree.
 */
export function useSchedulerStatus(applicationId: string): { status: SchedulerStatus | null; fetchedAt: number } {
  const { data, dataUpdatedAt } = useQuery({
    queryKey: ["schedulerStatus", applicationId],
    queryFn: async (): Promise<SchedulerStatus | null> => {
      try {
        return JSON.parse(bytesToText(await readApplicationFile(applicationId, STATUS_PATH))) as SchedulerStatus;
      } catch {
        return null;
      }
    },
    enabled: applicationId.length > 0,
    // A server without the plugin is looked at again once a minute rather
    // than every few seconds - see `useMinecraftStatus` for why the churn matters.
    refetchInterval: (query) => (query.state.data ? POLL_INTERVALS.applicationDetail : ABSENT_RECHECK_MS),
    retry: false,
  });

  return { status: data && data.schema === 1 ? data : null, fetchedAt: dataUpdatedAt };
}
