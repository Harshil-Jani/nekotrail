import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";

// In the Mac app the window is transparent so the sidebar gets real vibrancy; the browser preview keeps solid colours.
if ("__TAURI_INTERNALS__" in window && navigator.userAgent.includes("Mac")) document.documentElement.classList.add("native");

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
