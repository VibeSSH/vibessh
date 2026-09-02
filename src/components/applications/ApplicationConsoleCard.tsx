import { useCallback, useEffect, useRef, useState, type FormEvent } from "react";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { useTranslation } from "react-i18next";
import { POLL_INTERVALS, usePolling } from "@/hooks/usePolling";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { Icon } from "@/components/ui/Icon";
import {
  followApplicationLogs,
  getApplicationLogs,
  onApplicationLogClosed,
  onApplicationLogLine,
  stopFollowingApplicationLogs,
  writeApplicationConsole,
} from "@/services/applicationService";
import "@/components/servers/forms.css";
import "./ApplicationConsoleCard.css";
import { errorMessage } from "@/services/tauri";

interface ApplicationConsoleCardProps {
  applicationId: string;
  isRunning: boolean;
}

const TAIL_LINES = 200;

/**
 * A console on the Overview tab, not buried in the read-only Logs tab.
 *
 * Live where it can be: a Docker Application over SSH gets a real
 * `docker logs -f` stream, so output arrives as the container produces it
 * rather than up to one poll interval later. Everything else - a local
 * process, a systemd unit - falls back to the two-second `tail()` poll this
 * card used to do for everyone. The fallback is the old behaviour, not a
 * degraded mode, and the two never run at once: polling is disabled the
 * moment a stream is live, or the same lines would arrive twice.
 *
 * The stream is stopped on unmount. That is not tidiness - the handle is
 * what closes the SSH channel, and leaking it leaves `docker logs -f`
 * running on somebody's Node. The input row disables
 * itself with a one-time explanation the first time a write comes back as
 * "no console" or read-only (a systemd unit with no stdin, see
 * `runtime::ApplicationConsole`'s own doc comment) instead of letting the
 * user keep retrying into the same dead end.
 */
export function ApplicationConsoleCard({ applicationId, isRunning }: ApplicationConsoleCardProps) {
  const { t } = useTranslation();
  const [lines, setLines] = useState<string[]>([]);
  const [input, setInput] = useState("");
  const [sending, setSending] = useState(false);
  const [unsupported, setUnsupported] = useState<string | null>(null);
  const outputRef = useRef<HTMLPreElement>(null);
  const stickToBottom = useRef(true);

  /**
   * Which source is filling the output, and the reason this is three states
   * rather than a boolean.
   *
   * With a boolean starting at `false`, polling was enabled from the first
   * render: it fetched the 200-line tail, and then the stream - which asks
   * for `--tail 200` so a console does not open empty - replayed the same
   * 200 lines and appended them. Every line appeared twice. `deciding`
   * holds polling back until the follow has either started or failed, so
   * exactly one source ever fills the buffer.
   */
  const [source, setSource] = useState<"deciding" | "stream" | "poll">("deciding");
  const streaming = source === "stream";

  const poll = useCallback(async () => {
    try {
      setLines(await getApplicationLogs(applicationId, TAIL_LINES));
    } catch {
      // The Overview tab already surfaces the application's own load error
      // elsewhere - a failed poll here just leaves the last-known output in
      // place rather than piling on a second error banner.
    }
  }, [applicationId]);

  // Only while there is no stream. Running both would deliver every line
  // twice: the follow pushes it, and the next poll re-reads the same tail.
  usePolling(poll, POLL_INTERVALS.console, { enabled: source === "poll" });

  useEffect(() => {
    let cancelled = false;
    const followId = crypto.randomUUID();
    const unlisteners: UnlistenFn[] = [];

    async function start() {
      const offLine = await onApplicationLogLine(followId, (line) => {
        // Capped the same way the polled view is: a container in a crash
        // loop can produce output faster than anyone reads it, and an
        // unbounded array is a memory leak with a scrollbar.
        setLines((previous) => {
          const next = [...previous, line];
          return next.length > TAIL_LINES ? next.slice(next.length - TAIL_LINES) : next;
        });
      });
      // A stream that ends - the container stopped, the connection dropped -
      // hands the console back to polling rather than leaving it frozen on
      // the last line it happened to receive.
      const offClosed = await onApplicationLogClosed(followId, () => setSource("poll"));
      unlisteners.push(offLine, offClosed);

      try {
        await followApplicationLogs(applicationId, followId, TAIL_LINES);
        if (cancelled) {
          // The effect was torn down while this was in flight - React's
          // StrictMode does exactly that on every mount in development, and
          // a real unmount does it whenever the tab changes. The cleanup
          // already ran and stopped an id that was not registered yet, so
          // without this the follow registers a moment later with nobody
          // left to stop it: `docker logs -f` running on the Node for the
          // life of the process.
          void stopFollowingApplicationLogs(followId).catch(() => {});
          return;
        }
        setSource("stream");
      } catch (err) {
        // Polling stays on, which is what this card always did - but the
        // reason goes to the console. A runtime with no follow is normal
        // and silent; a Docker Application that should have streamed and
        // did not is a fault, and swallowing both identically made "why is
        // this still polling?" unanswerable.
        console.warn("Vibe console: live output unavailable, falling back to polling", err);
        if (!cancelled) setSource("poll");
      }
    }

    void start();

    return () => {
      cancelled = true;
      unlisteners.forEach((off) => off());
      // Fire and forget: the component is going away either way, and the
      // backend treats an unknown id as a no-op.
      void stopFollowingApplicationLogs(followId).catch(() => {});
      setSource("deciding");
    };
  }, [applicationId]);

  useEffect(() => {
    if (stickToBottom.current && outputRef.current) {
      outputRef.current.scrollTop = outputRef.current.scrollHeight;
    }
  }, [lines]);

  function handleOutputScroll() {
    const el = outputRef.current;
    if (!el) return;
    stickToBottom.current = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
  }

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    const trimmed = input.trim();
    if (!trimmed || sending) return;
    setSending(true);
    try {
      await writeApplicationConsole(applicationId, trimmed);
      setInput("");
      stickToBottom.current = true;
      if (!streaming) {
        setLines(await getApplicationLogs(applicationId, TAIL_LINES));
      }
    } catch (err) {
      setUnsupported(errorMessage(err, t));
    } finally {
      setSending(false);
    }
  }

  return (
    <Card>
      <div className="card-header application-console-header">
        <span className="application-console-dots" aria-hidden="true">
          <span className="application-console-dot application-console-dot-red" />
          <span className="application-console-dot application-console-dot-yellow" />
          <span className="application-console-dot application-console-dot-green" />
        </span>
        <h3 className="card-title">{t("applicationConsole.title")}</h3>
        <span className="application-console-mode">
          {source === "stream" ? t("applicationConsole.live") : source === "poll" ? t("applicationConsole.polled") : t("applicationConsole.connecting")}
        </span>
      </div>
      <pre className="application-console-output" ref={outputRef} onScroll={handleOutputScroll}>
        {lines.length === 0 ? t("applicationConsole.empty") : lines.join("\n")}
      </pre>
      {unsupported ? (
        <p className="form-note form-note-danger form-note-spaced">{unsupported}</p>
      ) : (
        <form className="application-console-input-row" onSubmit={handleSubmit}>
          <input
            className="form-input"
            value={input}
            onChange={(e) => setInput(e.target.value)}
            placeholder={isRunning ? t("applicationConsole.placeholder") : t("applicationConsole.notRunning")}
            disabled={!isRunning || sending}
          />
          <Button type="submit" size="sm" disabled={!isRunning || sending || !input.trim()}>
            <Icon name="chevron-right" size={14} />
            {t("applicationConsole.send")}
          </Button>
        </form>
      )}
    </Card>
  );
}
