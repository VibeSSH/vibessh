import { useEffect, useState } from "react";
import { getAiConfig } from "@/services/aiService";

/**
 * Whether the assistant is configured enough to answer.
 *
 * Used to decide whether an "Ask Vibe AI" button appears at all. Offering
 * help at the moment something is already broken, and having that help
 * resolve to "first go and configure an API key", is worse than not
 * offering it - so the entry points stay hidden until the feature can
 * actually do something.
 *
 * Starts `false` rather than `null`: the button is absent while this
 * resolves, which is a fraction of a second and avoids one appearing and
 * then vanishing under the pointer.
 */
export function useAiReady(): boolean {
  const [ready, setReady] = useState(false);

  useEffect(() => {
    let cancelled = false;
    getAiConfig()
      .then((config) => {
        if (!cancelled) setReady(config.enabled && config.baseUrl.length > 0 && config.model.length > 0);
      })
      .catch(() => {
        if (!cancelled) setReady(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  return ready;
}
