import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { ConfigProvider } from "./hooks/useConfig";
import { SessionMetaProvider } from "./hooks/useSessionMeta";
import "./index.css";
import "@xterm/xterm/css/xterm.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <ConfigProvider>
      <SessionMetaProvider>
        <App />
      </SessionMetaProvider>
    </ConfigProvider>
  </React.StrictMode>
);
