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
import { logLevelOf } from "./logLevel";

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
  // Read by the reconnect below, which lives inside an effect and would
  // otherwise see the line count as it was when that effect first ran.
  const lineCountRef = useRef(0);
  /** Why the last read failed, shown only while nothing has ever arrived. */
  const [readFailure, setReadFailure] = useState<string | null>(null);
  const streaming = source === "stream";

  const poll = useCallback(async () => {
    try {
      const fetched = await getApplicationLogs(applicationId, TAIL_LINES);
      lineCountRef.current = fetched.length;
      setLines(fetched);
      setReadFailure(null);
    } catch (err) {
      // A failed poll leaves the last-known output in place rather than
      // piling on a second error banner - the Overview tab already surfaces
      // the application's own load error. But when nothing has *ever*
      // arrived, that silence renders as "no logs yet", which is a different
      // claim and a false one. The reason is kept for that case only.
      setReadFailure(errorMessage(err, t));
    }
  }, [applicationId, t]);

  // Only while there is no stream. Running both would deliver every line
  // twice: the follow pushes it, and the next poll re-reads the same tail.
  usePolling(poll, POLL_INTERVALS.console, { enabled: source === "poll" });

  useEffect(() => {
    let cancelled = false;
    let live: string | null = null;
    let unlisteners: UnlistenFn[] = [];
    let attempt = 0;
    let retryTimer: number | undefined;

    const dropListeners = () => {
      unlisteners.forEach((off) => off());
      unlisteners = [];
    };

    /**
     * Opens one stream.
     *
     * `seedTail` is the history to ask the Node for. Full on the first
     * connect, and on any reconnect that has nothing on screen; zero only
     * when there is already a window to append to, since replaying it would
     * repeat the whole thing each time the transport blinked.
     *
     * That condition used to be "is this a reconnect", which is not the same
     * question: a stream that dies before delivering a line leaves an empty
     * buffer, and asking for zero then shows nothing at all until the
     * container writes something new - on a server that has finished
     * booting, that is a console labelled "live" and containing nothing.
     */
    async function open(seedTail: number) {
      const followId = crypto.randomUUID();
      live = followId;

      const offLine = await onApplicationLogLine(followId, (line) => {
        // Capped the same way the polled view is: a container in a crash
        // loop can produce output faster than anyone reads it, and an
        // unbounded array is a memory leak with a scrollbar.
        setLines((previous) => {
          const next = [...previous, line];
          const capped = next.length > TAIL_LINES ? next.slice(next.length - TAIL_LINES) : next;
          lineCountRef.current = capped.length;
          return capped;
        });
      });

      const offClosed = await onApplicationLogClosed(followId, () => {
        // A stream ending is not the end of the console.
        //
        // Measured on a running container: the follow delivered 232 lines
        // and then stopped six seconds later, because anything that hits a
        // connection error drops the cached SSH session and reconnects -
        // and every channel on it, this one included, dies with it. Falling
        // back to polling permanently turned a blink into a downgrade that
        // lasted until the tab was reopened.
        if (cancelled || live !== followId) return;
        reconnect();
      });

      unlisteners.push(offLine, offClosed);

      try {
        await followApplicationLogs(applicationId, followId, seedTail);
        if (cancelled) {
          // The effect was torn down while this was in flight - React's
          // StrictMode does exactly that on every mount in development, and
          // a real unmount does it whenever the tab changes. Without this
          // the follow registers a moment later with nobody left to stop
          // it: `docker logs -f` running on the Node for the life of the
          // process.
          void stopFollowingApplicationLogs(applicationId).catch(() => {});
          return;
        }
        attempt = 0;
        setSource("stream");
      } catch (err) {
        // A runtime with no follow is normal and permanent - a local
        // process, a systemd unit - so this does not retry. Polling is what
        // this card always did, and the reason goes to the browser console
        // so "why is this still polling?" is answerable.
        console.warn("Vibe console: live output unavailable, falling back to polling", err);
        if (!cancelled) setSource("poll");
      }
    }

    /**
     * Reopens after a stream ended by itself, backing off.
     *
     * Bounded because not every ending is transient: a container that has
     * stopped will end every follow immediately, and retrying forever would
     * be a request per second against a Node for output that is not coming.
     * After the last attempt the console keeps working, on the two-second
     * poll it used before any of this existed.
     */
    function reconnect() {
      dropListeners();
      if (attempt >= 4) {
        setSource("poll");
        return;
      }
      const delay = 500 * 2 ** attempt;
      attempt += 1;
      setSource("deciding");
      retryTimer = window.setTimeout(() => {
        // Nothing on screen means nothing to duplicate, so ask for history.
        if (!cancelled) void open(lineCountRef.current > 0 ? 0 : TAIL_LINES);
      }, delay);
    }

    void open(TAIL_LINES);

    return () => {
      cancelled = true;
      window.clearTimeout(retryTimer);
      dropListeners();
      // Fire and forget: the component is going away either way, and the
      // backend treats an unknown id as a no-op.
      void stopFollowingApplicationLogs(applicationId).catch(() => {});
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
        {lines.length === 0
          ? (readFailure ?? t("applicationConsole.empty"))
          : // One element per line rather than one joined string, so each can
            // carry its own severity. Keyed by index because these lines are
            // an append-only window with no identity of their own - two
            // identical lines are genuinely two events, not one repeated.
            lines.map((line, index) => (
              <span key={index} className={`application-console-line application-console-line-${logLevelOf(line)}`}>
                {line}
                {"\n"}
              </span>
            ))}
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
