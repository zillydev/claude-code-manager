import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { ConfigProvider } from "./hooks/useConfig";
import { SessionMetaProvider } from "./hooks/useSessionMeta";
import { WindowViewProvider } from "./hooks/useWindowView";
import "./index.css";
import "@xterm/xterm/css/xterm.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <ConfigProvider>
      <WindowViewProvider>
        <SessionMetaProvider>
          <App />
        </SessionMetaProvider>
      </WindowViewProvider>
    </ConfigProvider>
  </React.StrictMode>
);
