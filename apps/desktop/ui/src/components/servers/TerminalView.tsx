import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon } from "@xterm/addon-search";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { WebglAddon } from "@xterm/addon-webgl";
import { ClipboardAddon } from "@xterm/addon-clipboard";
import "@xterm/xterm/css/xterm.css";
import { Icon } from "@/components/ui/Icon";
import {
  closeTerminal,
  onTerminalClosed,
  onTerminalOutput,
  openTerminal,
  resizeTerminal,
  writeToTerminal,
} from "@/services/terminalService";
import { errorMessage } from "@/services/tauri";
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
 *
 * Addon set matches Voltius's own terminal (fit/search/web-links/webgl/
 * clipboard - voltius's package.json lists the same five @xterm/addon-*
 * packages): web-links makes URLs in output clickable, clipboard wires up
 * OSC 52 so remote programs (tmux, vim) can set the local clipboard, webgl
 * is GPU-accelerated rendering with a graceful fallback to the default
 * canvas renderer on context loss, and search backs the Ctrl+F bar below.
 */
/**
 * The terminal's palette, read from the tokens the rest of the interface uses.
 *
 * Was three hardcoded hex values, which meant the terminal stayed the old
 * navy while every surface around it changed colour. The fallbacks are the
 * shipped palette, for the moment before a theme has been applied.
 */
function readTerminalTheme() {
  const styles = getComputedStyle(document.documentElement);
  const token = (name: string, fallback: string) => styles.getPropertyValue(name).trim() || fallback;
  return {
    background: token("--surface-bg", "#090a0c"),
    foreground: token("--text-primary", "#ecedef"),
    cursor: token("--accent", "#4dd9f5"),
    selectionBackground: token("--surface-3", "#1d2026"),
  };
}

export function TerminalView({ serverId, onClosed }: TerminalViewProps) {
  const { t } = useTranslation();
  const containerRef = useRef<HTMLDivElement>(null);
  const searchAddonRef = useRef<SearchAddon | null>(null);
  // Held in a ref so the right-click handler below reaches the paste that
  // belongs to the terminal instance currently mounted, rather than closing
  // over a stale one.
  const pasteRef = useRef<(() => void) | null>(null);
  const searchInputRef = useRef<HTMLInputElement>(null);
  const [searchOpen, setSearchOpen] = useState(false);
  const [searchTerm, setSearchTerm] = useState("");

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const term = new Terminal({
      cursorBlink: true,
      fontFamily: "'JetBrains Mono', Consolas, 'SF Mono', monospace",
      fontSize: 13,
      theme: readTerminalTheme(),
    });

    // xterm paints into its own canvas, so it cannot inherit a CSS variable
    // the way everything else does - the colours have to be handed to it.
    // Watching the root element's inline style is what keeps it in step:
    // that is exactly where `applyTheme` writes, so switching theme repaints
    // an open terminal instead of leaving it in the previous palette until
    // it is reopened.
    const themeWatcher = new MutationObserver(() => {
      term.options.theme = readTerminalTheme();
    });
    themeWatcher.observe(document.documentElement, { attributes: true, attributeFilter: ["style"] });

    const fitAddon = new FitAddon();
    term.loadAddon(fitAddon);

    const searchAddon = new SearchAddon();
    term.loadAddon(searchAddon);
    searchAddonRef.current = searchAddon;

    term.loadAddon(new WebLinksAddon());
    term.loadAddon(new ClipboardAddon());

    // WebGL context can be lost (GPU driver reset, too many contexts) -
    // xterm's own recommended pattern is to dispose and let it fall back to
    // the default canvas renderer rather than leaving the terminal broken.
    try {
      const webglAddon = new WebglAddon();
      webglAddon.onContextLoss(() => webglAddon.dispose());
      term.loadAddon(webglAddon);
    } catch {
      // WebGL unavailable in this environment - canvas rendering still works.
    }

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
      term.writeln(t("terminalPage.connecting"));
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
          term.write(`\r\n\x1b[31m[${reason ? t("terminalPage.disconnectedReason", { reason }) : t("terminalPage.disconnected")}]\x1b[0m\r\n`);
          onClosed?.(reason);
        });
      } catch (err) {
        // Through `errorMessage`, like every other surface: the raw message
        // is the backend's English, which is what printed "invalid input:
        // SSH authentication was rejected" under a Polish frame.
        term.writeln(`\x1b[31m${t("terminalPage.failedToOpen", { error: errorMessage(err, t) })}\x1b[0m`);
      }
    }
    start();

    const dataDisposable = term.onData((data) => {
      if (terminalId) writeToTerminal(terminalId, data).catch(() => {});
    });

    /**
     * Puts the local clipboard into the terminal.
     *
     * `term.paste` rather than writing the text as input: it applies
     * bracketed-paste mode when the remote program asked for it, which is
     * what stops an editor from auto-indenting every line of a pasted block
     * into a staircase.
     */
    const pasteFromClipboard = async () => {
      try {
        const text = await navigator.clipboard.readText();
        if (text) term.paste(text);
      } catch {
        // Reading the clipboard can be refused, and a terminal that silently
        // ignores a paste is indistinguishable from a broken one - so it
        // says so in the terminal itself, where the person is looking.
        term.write("\r\n\x1b[33m" + t("terminalPage.pasteBlocked") + "\x1b[0m\r\n");
      }
    };
    pasteRef.current = pasteFromClipboard;

    term.attachCustomKeyEventHandler((event) => {
      if (event.type === "keydown" && event.ctrlKey && event.key.toLowerCase() === "f") {
        setSearchOpen(true);
        queueMicrotask(() => searchInputRef.current?.focus());
        return false;
      }
      // The terminal conventions. Deliberately the shifted pair: plain
      // Ctrl+C has to keep reaching the shell as "interrupt", which is the
      // whole reason terminals moved copy and paste onto Ctrl+Shift.
      if (event.type === "keydown" && event.ctrlKey && event.shiftKey && event.key.toLowerCase() === "v") {
        void pasteFromClipboard();
        return false;
      }
      if (event.type === "keydown" && event.ctrlKey && event.shiftKey && event.key.toLowerCase() === "c") {
        const selection = term.getSelection();
        if (selection) void navigator.clipboard.writeText(selection).catch(() => {});
        return false;
      }
      if (event.type === "keydown" && event.key === "Escape") {
        setSearchOpen(false);
        return false;
      }
      return true;
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
      themeWatcher.disconnect();
      term.dispose();
      searchAddonRef.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [serverId]);

  function runSearch(direction: "next" | "previous") {
    if (!searchTerm) return;
    const addon = searchAddonRef.current;
    if (!addon) return;
    if (direction === "next") addon.findNext(searchTerm);
    else addon.findPrevious(searchTerm);
  }

  return (
    <div className="terminal-view-wrap">
      {searchOpen && (
        <div className="terminal-search-bar">
          <Icon name="search" size={14} />
          <input
            ref={searchInputRef}
            className="terminal-search-input"
            placeholder={t("terminalPage.searchPlaceholder")}
            value={searchTerm}
            onChange={(e) => {
              setSearchTerm(e.target.value);
              if (e.target.value) searchAddonRef.current?.findNext(e.target.value, { incremental: true });
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter") runSearch(e.shiftKey ? "previous" : "next");
              if (e.key === "Escape") setSearchOpen(false);
            }}
          />
          <button className="terminal-search-btn" onClick={() => runSearch("previous")} aria-label={t("terminalPage.searchPrevAria")}>
            <Icon name="chevron-left" size={14} />
          </button>
          <button className="terminal-search-btn" onClick={() => runSearch("next")} aria-label={t("terminalPage.searchNextAria")}>
            <Icon name="chevron-right" size={14} />
          </button>
          <button className="terminal-search-btn" onClick={() => setSearchOpen(false)} aria-label={t("terminalPage.searchCloseAria")}>
            <Icon name="x" size={14} />
          </button>
        </div>
      )}
      {/* Right-click pastes, the way PuTTY and WinSCP do. It is the
        * binding people reach for first, and the one whose absence sends
        * them back to retyping a command by hand. */}
      <div
        ref={containerRef}
        className="terminal-view"
        onContextMenu={(event) => {
          event.preventDefault();
          pasteRef.current?.();
        }}
      />
    </div>
  );
}
