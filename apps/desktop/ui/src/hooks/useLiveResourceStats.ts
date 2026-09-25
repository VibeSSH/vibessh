import { useEffect, useRef, useState } from "react";
import type { UnlistenFn } from "@tauri-apps/api/event";
import {
  followApplicationStats,
  onApplicationStatsClosed,
  onApplicationStatsSample,
  stopFollowingApplicationStats,
  type ResourceStatsSample,
} from "@/services/applicationService";

/** How long to wait before reopening a stream that ended while it was still wanted. */
const RETRY_AFTER_MS = 5000;

/**
 * An Application's CPU and memory as a live stream - `docker stats` on the
 * Node, about one reading a second - instead of a poll every few seconds.
 *
 * `streaming` is true once the first reading has arrived and until the stream
 * ends. While it is false the page keeps polling exactly as before, which is
 * also what happens for anything that cannot stream (not Docker, an
 * Agent-mode Node): the backend refuses, and the refusal is the fallback.
 *
 * A stream that ends while still wanted - a blip in the connection - is
 * reopened after a pause rather than left on the slower poll for the rest of
 * the visit.
 */
export function useLiveResourceStats(
  applicationId: string | undefined,
  enabled: boolean,
  onSample: (sample: ResourceStatsSample) => void,
): { streaming: boolean } {
  const [streaming, setStreaming] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const onSampleRef = useRef(onSample);
  onSampleRef.current = onSample;

  useEffect(() => {
    if (!applicationId || !enabled) {
      setStreaming(false);
      return;
    }
    const streamId = crypto.randomUUID();
    let active = true;
    let retryTimer: number | undefined;

    const scheduleRetry = () => {
      if (!active) return;
      setStreaming(false);
      retryTimer = window.setTimeout(() => active && setAttempt((n) => n + 1), RETRY_AFTER_MS);
    };

    // Listening before starting, so the first reading cannot arrive before
    // anybody is there to hear it.
    const listeners: Promise<UnlistenFn>[] = [
      onApplicationStatsSample(streamId, (sample) => {
        if (!active) return;
        setStreaming(true);
        onSampleRef.current(sample);
      }),
      onApplicationStatsClosed(streamId, scheduleRetry),
    ];
    void Promise.all(listeners).then(() => {
      if (!active) return;
      // A refusal is the "cannot stream this" answer: stay on the poll, and
      // do not retry something that will refuse the same way.
      followApplicationStats(applicationId, streamId).catch(() => active && setStreaming(false));
    });

    return () => {
      active = false;
      window.clearTimeout(retryTimer);
      setStreaming(false);
      for (const listener of listeners) void listener.then((unlisten) => unlisten());
      // Named by stream, so a remount's fresh stream is not the one ended.
      stopFollowingApplicationStats(applicationId, streamId).catch(() => {});
    };
  }, [applicationId, enabled, attempt]);

  return { streaming };
}
