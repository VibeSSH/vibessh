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
        // The hosted provider answers with no base URL or model of its own -
        // those fields belong to an OpenAI-compatible endpoint. Requiring them
        // for every provider hid every "Ask Vibe AI" button from anyone on the
        // included model, which is the default a fresh install opens on.
        if (!cancelled) {
          const configured = config.provider === "vibeSshHosted" || (config.baseUrl.length > 0 && config.model.length > 0);
          setReady(config.enabled && configured);
        }
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
