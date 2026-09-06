import { useRef, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { Icon } from "@/components/ui/Icon";
import { runApplicationCommand } from "@/services/applicationService";
import { errorMessage } from "@/services/tauri";
import type { Blueprint } from "@/types/application";
import "@/components/servers/forms.css";
import "./CommandConsoleCard.css";

interface CommandConsoleCardProps {
  applicationId: string;
  blueprint: Blueprint | null;
}

/** One command and what came back. Kept as a list so the answer stays next to
 * the question that produced it - a single output box loses which command a
 * given wall of text belongs to as soon as the second one is sent. */
interface Exchange {
  id: number;
  command: string;
  answer: string;
  failed: boolean;
}

/** More than anybody reads back, and low enough that a session spent poking
 * at a database does not grow a page without bound. */
const MAX_EXCHANGES = 50;

/**
 * Asks a database something.
 *
 * **Why this is not the console on the Overview tab.** That one writes to the
 * container's stdin, which is how a Minecraft server takes commands.
 * `redis-server` and `mongod` ignore stdin, so pointing that console at them
 * would give an input box that swallows everything typed into it. This runs
 * the server's own client instead - `redis-cli`, `mongosh` - and shows what
 * it printed.
 *
 * **Each command is its own run**, and the card says so rather than letting
 * somebody discover it: `use some-database` applies to the command it was
 * typed with and to nothing after it.
 *
 * Renders nothing for a blueprint that declares no console, which is most of
 * them - the shape of a client command is per-kind knowledge and lives on the
 * blueprint, not here.
 */
export function CommandConsoleCard({ applicationId, blueprint }: CommandConsoleCardProps) {
  const { t } = useTranslation();
  const [command, setCommand] = useState("");
  const [exchanges, setExchanges] = useState<Exchange[]>([]);
  const [busy, setBusy] = useState(false);
  const nextId = useRef(1);
  /**
   * Commands already sent, newest last, walked with the arrow keys.
   *
   * A console without history is one where a typo means retyping the whole
   * line, which is the difference between poking at a database and giving
   * up. `null` means "not walking it" - the position resets on every send.
   */
  const history = useRef<string[]>([]);
  const historyAt = useRef<number | null>(null);

  const spec = blueprint?.commandConsole;
  if (!spec) return null;

  async function send(event: FormEvent) {
    event.preventDefault();
    const typed = command.trim();
    if (typed === "" || busy) return;

    setBusy(true);
    history.current.push(typed);
    historyAt.current = null;
    setCommand("");
    try {
      const answer = await runApplicationCommand(applicationId, typed);
      push({ command: typed, answer: answer.trim() === "" ? t("commandConsole.noOutput") : answer, failed: false });
    } catch (err) {
      // Shown in the transcript rather than as a banner: a refused command is
      // an answer to the command above it, and belongs with it.
      push({ command: typed, answer: errorMessage(err, t), failed: true });
    } finally {
      setBusy(false);
    }
  }

  function push(exchange: Omit<Exchange, "id">) {
    setExchanges((previous) => [...previous, { ...exchange, id: nextId.current++ }].slice(-MAX_EXCHANGES));
  }

  function walkHistory(direction: -1 | 1) {
    if (history.current.length === 0) return;
    const last = history.current.length - 1;
    const current = historyAt.current ?? history.current.length;
    const next = Math.min(Math.max(current + direction, 0), history.current.length);
    historyAt.current = next;
    setCommand(next > last ? "" : history.current[next]);
  }

  return (
    <Card title={t("commandConsole.title")}>
      <p className="form-note">{t("commandConsole.intro")}</p>

      {exchanges.length > 0 && (
        <div className="command-console-transcript">
          {exchanges.map((exchange) => (
            <div key={exchange.id} className="command-console-exchange">
              <p className="command-console-command">
                <span className="command-console-prompt">&gt;</span> {exchange.command}
              </p>
              <pre className={`command-console-answer ${exchange.failed ? "command-console-answer-failed" : ""}`}>{exchange.answer}</pre>
            </div>
          ))}
        </div>
      )}

      <form className="command-console-form" onSubmit={(event) => void send(event)}>
        <input
          className="form-input command-console-input"
          value={command}
          onChange={(event) => setCommand(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "ArrowUp") {
              event.preventDefault();
              walkHistory(-1);
            } else if (event.key === "ArrowDown") {
              event.preventDefault();
              walkHistory(1);
            }
          }}
          placeholder={spec.placeholder}
          spellCheck={false}
          autoComplete="off"
          aria-label={t("commandConsole.title")}
        />
        <Button type="submit" disabled={busy || command.trim() === ""}>
          <Icon name="chevron-right" size={14} />
          {busy ? t("commandConsole.running") : t("commandConsole.run")}
        </Button>
      </form>
    </Card>
  );
}
