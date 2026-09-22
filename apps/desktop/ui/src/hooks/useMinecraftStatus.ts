import { useEffect, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { POLL_INTERVALS } from "@/hooks/usePolling";
import { readApplicationFile } from "@/services/applicationFilesService";
import { bytesToText } from "@/services/filesService";

/** Mirrors the VibeSSH Metrics plugin's `status.json` - see the plugin's `ServerStatus`. */
export interface McStatus {
  schema: number;
  updatedAt: string;
  server: { version: string; software: string; uptimeSeconds: number };
  tps: { m1: number; m5: number; m15: number };
  msptAvg: number;
  players: { online: number; max: number; names?: string[] | null };
  memory: { usedMb: number; maxMb: number };
  worlds: { name: string; players: number; entities: number; chunks: number }[];
}

/** One poll's worth of the numbers the tab charts over time. */
export interface McSample {
  tps: number;
  mspt: number;
  players: number;
  ramPercent: number;
}

/** Where the VibeSSH Metrics plugin writes, relative to the application's working directory. */
const STATUS_PATH = ".vibessh/status.json";
/** How many samples the sparklines keep - five minutes at the detail poll. */
const HISTORY = 60;

/**
 * The live Minecraft status of an application, or null when it is not running the
 * VibeSSH Metrics plugin.
 *
 * Drives both whether the Minecraft tab is shown at all and what it renders, so the tab
 * appears exactly for the servers that can fill it. Reads `status.json` over the same SSH
 * connection the rest of the page uses, on the detail poll; a missing file is "no Minecraft
 * here", not an error. Keeps a short rolling TPS history for the tab's sparkline.
 */
export function useMinecraftStatus(applicationId: string): { status: McStatus | null; history: McSample[] } {
  const { data } = useQuery({
    queryKey: ["minecraftStatus", applicationId],
    queryFn: async (): Promise<McStatus | null> => {
      try {
        return JSON.parse(bytesToText(await readApplicationFile(applicationId, STATUS_PATH))) as McStatus;
      } catch {
        return null;
      }
    },
    enabled: applicationId.length > 0,
    refetchInterval: POLL_INTERVALS.applicationDetail,
    retry: false,
  });

  const [history, setHistory] = useState<McSample[]>([]);
  const lastStamp = useRef<string | null>(null);
  useEffect(() => {
    if (!data || data.updatedAt === lastStamp.current) return;
    lastStamp.current = data.updatedAt;
    const ramPercent = data.memory.maxMb > 0 ? (data.memory.usedMb / data.memory.maxMb) * 100 : 0;
    setHistory((previous) =>
      [...previous, { tps: data.tps.m1, mspt: data.msptAvg, players: data.players.online, ramPercent }].slice(-HISTORY),
    );
  }, [data]);

  return { status: data && data.schema === 1 ? data : null, history };
}
