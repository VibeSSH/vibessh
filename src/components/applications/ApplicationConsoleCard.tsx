import { useEffect, useRef, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { Icon } from "@/components/ui/Icon";
import { getApplicationLogs, writeApplicationConsole } from "@/services/applicationService";
import "@/components/servers/forms.css";
import "./ApplicationConsoleCard.css";

interface ApplicationConsoleCardProps {
  applicationId: string;
  isRunning: boolean;
}

const POLL_INTERVAL_MS = 2000;
const TAIL_LINES = 200;

/**
 * A console on the Overview tab, not buried in the read-only Logs tab -
 * polls the same `tail()` snapshot the Logs tab uses (there's no
 * push-based output streaming yet, see `applicationService.getApplicationLogs`'s
 * own doc comment) but on a shorter interval so typing a command and
 * watching the reaction still feels close to live. The input row disables
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

  useEffect(() => {
    let cancelled = false;
    async function poll() {
      try {
        const next = await getApplicationLogs(applicationId, TAIL_LINES);
        if (!cancelled) setLines(next);
      } catch {
        // The Overview tab already surfaces the application's own load
        // error elsewhere - a failed poll here just leaves the last-known
        // output in place rather than piling on a second error banner.
      }
    }
    poll();
    const intervalId = window.setInterval(poll, POLL_INTERVAL_MS);
    return () => {
      cancelled = true;
      window.clearInterval(intervalId);
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
      const next = await getApplicationLogs(applicationId, TAIL_LINES);
      setLines(next);
    } catch (err) {
      setUnsupported(err instanceof Error ? err.message : t("applicationConsole.writeError"));
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
