import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWindow } from "@tauri-apps/api/window";
import App from "./App";
import { setThemeBackend, setZoomBackend, startAppearance } from "./appearance";
import { getSettings } from "./settings";
import { IS_MAC } from "./platform";

// In the Mac app the window is transparent so the sidebar gets real vibrancy; the browser preview keeps solid colours.
if ("__TAURI_INTERNALS__" in window && IS_MAC && navigator.userAgent.includes("Mac")) document.documentElement.classList.add("native");
// Linux has a normal title bar (no traffic lights inside the sidebar) and solid colours.
if (!IS_MAC) document.documentElement.classList.add("linux");

// The app zooms the web view itself and sets the window's light or dark; a plain browser falls back to CSS.
if ("__TAURI_INTERNALS__" in window) {
  setZoomBackend((z) => getCurrentWebview().setZoom(z));
  setThemeBackend((t) => getCurrentWindow().setTheme(t));
}
startAppearance(getSettings);

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
