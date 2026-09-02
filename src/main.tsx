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

// A desktop app has no business showing the WebView's own native menu
// (Back/Reload/Save As/Print/Inspect Element) - nothing here intercepted
// right-click before this, so every element fell through to it. Global,
// not per-component: a row that wants its own context menu (the Files
// browser) still gets one - it just renders through React state from its
// own onContextMenu handler, entirely separate from this native fallback.
document.addEventListener("contextmenu", (e) => e.preventDefault());

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <QueryClientProvider client={queryClient}>
      <HashRouter>
        <App />
      </HashRouter>
    </QueryClientProvider>
  </React.StrictMode>,
);
