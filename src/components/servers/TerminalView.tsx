import { useEffect, useRef } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import {
  closeTerminal,
  onTerminalClosed,
  onTerminalOutput,
  openTerminal,
  resizeTerminal,
  writeToTerminal,
} from "@/services/terminalService";
import "./TerminalView.css";

interface TerminalViewProps {
  serverId: string;
  onClosed?: (reason: string | null) => void;
}

/**
 * One xterm.js instance wired to one backend terminal session for as long
 * as this component is mounted. Backend output arrives as raw chunks over a
 * per-terminal-id Tauri event (see terminalService) and is written straight
 * into xterm - no line buffering or parsing on this side, xterm handles the
 * ANSI escape sequences a real shell sends (colors, cursor movement, the
 * whole thing) the same way any other terminal emulator does.
 */
export function TerminalView({ serverId, onClosed }: TerminalViewProps) {
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const term = new Terminal({
      cursorBlink: true,
      fontFamily: "'JetBrains Mono', Consolas, 'SF Mono', monospace",
      fontSize: 13,
      theme: {
        background: "#0b1220",
        foreground: "#9effff",
        cursor: "#57c7d8",
      },
    });
    const fitAddon = new FitAddon();
    term.loadAddon(fitAddon);
    term.open(container);
    fitAddon.fit();

    let terminalId: string | null = null;
    // React 18 StrictMode mounts/cleans up/remounts every effect once in
    // dev - if cleanup runs while `openTerminal` is still in flight, this
    // makes the resolved id get closed immediately instead of left running
    // as an orphaned backend session nothing will ever clean up.
    let disposed = false;
    let unlistenOutput = () => {};
    let unlistenClosed = () => {};

    async function start() {
      term.writeln("Connecting...");
      try {
        const id = await openTerminal(serverId, term.cols, term.rows);
        if (disposed) {
          closeTerminal(id).catch(() => {});
          return;
        }
        terminalId = id;
        term.clear();

        unlistenOutput = await onTerminalOutput(id, (chunk) => term.write(chunk));
        unlistenClosed = await onTerminalClosed(id, (reason) => {
          term.write(`\r\n\x1b[31m[disconnected${reason ? `: ${reason}` : ""}]\x1b[0m\r\n`);
          onClosed?.(reason);
        });
      } catch (err) {
        term.writeln(`\x1b[31mFailed to open terminal: ${err instanceof Error ? err.message : String(err)}\x1b[0m`);
      }
    }
    start();

    const dataDisposable = term.onData((data) => {
      if (terminalId) writeToTerminal(terminalId, data).catch(() => {});
    });

    const resizeObserver = new ResizeObserver(() => {
      fitAddon.fit();
      if (terminalId) resizeTerminal(terminalId, term.cols, term.rows).catch(() => {});
    });
    resizeObserver.observe(container);

    return () => {
      disposed = true;
      resizeObserver.disconnect();
      dataDisposable.dispose();
      unlistenOutput();
      unlistenClosed();
      if (terminalId) closeTerminal(terminalId).catch(() => {});
      term.dispose();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [serverId]);

  return <div ref={containerRef} className="terminal-view" />;
}
