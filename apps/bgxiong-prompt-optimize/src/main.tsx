import React from "react";
import { createRoot } from "react-dom/client";
import { earlyHydrateTheme } from "../../../ui/src/theme/themeEarlyHydrate";
import "../../../ui/src/index.css";
import "../../../ui/src/styles/ui-edge-resize-shell.css";
import App from "./App";
import "./main.css";

// Apply the repo theme tokens without ThemeProvider's main-window IPC.
earlyHydrateTheme();

const root = document.getElementById("root");
if (!root) {
  throw new Error("missing #root");
}
createRoot(root).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
