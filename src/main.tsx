import React, { useEffect, useState } from "react";
import ReactDOM from "react-dom/client";
import { Theme } from "@radix-ui/themes";
import "@radix-ui/themes/styles.css";
import "./App.css";
import App from "./App";
import SettingsWindow from "./SettingsWindow";
import AskResultWindow from "./AskResultWindow";
import FloatingBarWindow from "./FloatingBarWindow";
import PageWindow from "./PageWindow";
import TrayPanelWindow from "./TrayPanelWindow";

const view = new URLSearchParams(window.location.search).get("view");

function AppTheme({ children }: { children: React.ReactNode }) {
  const [dark, setDark] = useState(() => window.matchMedia("(prefers-color-scheme: dark)").matches);

  useEffect(() => {
    const query = window.matchMedia("(prefers-color-scheme: dark)");
    const update = () => setDark(query.matches);
    query.addEventListener("change", update);
    return () => query.removeEventListener("change", update);
  }, []);

  return <Theme appearance={dark ? "dark" : "light"} accentColor="blue" grayColor="gray" hasBackground={false}>{children}</Theme>;
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <AppTheme>{view === "floating" ? <FloatingBarWindow />
      : view === "ask-result" ? <AskResultWindow />
      : view === "tray-panel" ? <TrayPanelWindow />
      : view === "settings" ? <SettingsWindow />
      : view === "history" || view === "dictionary" ? <PageWindow page={view} />
      : <App />}</AppTheme>
  </React.StrictMode>,
);
