import React from "react";
import ReactDOM from "react-dom/client";
import { HashRouter } from "react-router-dom";
import { QueryClientProvider } from "@tanstack/react-query";
import { queryClient } from "./services/queryClient";
import App from "./App";
import "@fontsource-variable/inter";
import "@fontsource/jetbrains-mono/400.css";
import "@fontsource/jetbrains-mono/500.css";
import "@fontsource/jetbrains-mono/600.css";
import "./styles/globals.css";
import "./i18n";
import { applyStoredTheme } from "@/theme/themeStore";
import { StartupFailureScreen } from "@/components/StartupFailureScreen";
import { getStartupFailure } from "@/services/appService";

// A desktop app has no business showing the WebView's own native menu
// (Back/Reload/Save As/Print/Inspect Element) - nothing here intercepted
// right-click before this, so every element fell through to it. Global,
// not per-component: a row that wants its own context menu (the Files
// browser) still gets one - it just renders through React state from its
// own onContextMenu handler, entirely separate from this native fallback.
// Shift is the escape hatch, the same one browsers use: holding it lets the
// native menu through, which is the only way to reach "Inspect" in a window
// with no menu bar. Without it, suppressing right-click also suppressed the
// developer's own way into the page.
// Before React mounts, so the window never paints once in the shipped palette
// and then again in the chosen one.
applyStoredTheme();

// Text fields are left alone here: `TextFieldContextMenu` gives them cut,
// copy and paste in the app's own menu, and suppressing the event first
// would make it look already handled.
document.addEventListener("contextmenu", (e) => {
  const target = e.target;
  const textField = target instanceof HTMLTextAreaElement || (target instanceof HTMLInputElement && !["checkbox", "radio", "button", "submit", "range", "color", "file"].includes(target.type));
  if (!e.shiftKey && !textField) e.preventDefault();
});

/**
 * The window's content when VibeSSH could not start. Rendered on its own,
 * outside the router and the query client: nothing the app needs was set up,
 * so the only thing to do is say why and how to get it to us.
 */
function renderStartupFailure(failure: NonNullable<Awaited<ReturnType<typeof getStartupFailure>>>) {
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <StartupFailureScreen failure={failure} />
    </React.StrictMode>,
  );
}

function render() {
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <QueryClientProvider client={queryClient}>
        <HashRouter>
          <App />
        </HashRouter>
      </QueryClientProvider>
    </React.StrictMode>,
  );
}

/**
 * Sample data for the guide's screenshots, and for looking at a screen
 * without a Node to hand.
 *
 * Dynamically imported behind `import.meta.env.DEV`, so the module is not in
 * a production build at all, and gated on `?fixtures=1`, so even in
 * development it does nothing unless the URL asks. It has to run before the
 * first render, because it is what defines the bridge every service call
 * goes through.
 */
async function start() {
  if (import.meta.env.DEV && new URLSearchParams(window.location.search).has("fixtures")) {
    const { installDevFixtures } = await import("./devFixtures");
    installDevFixtures();
  }
  // Asked before the app mounts, because the app mounting is what would
  // start calling commands that have nothing behind them. Anything that is
  // not a clear "it started" - no Tauri at all in a browser preview, say -
  // renders the app as before.
  const failure = await getStartupFailure().catch(() => null);
  if (failure) {
    renderStartupFailure(failure);
    return;
  }
  render();
}

void start();
