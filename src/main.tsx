import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import { useStore } from "./store";
import "./styles/tokens.css";
import "./styles/app.css";
import "./styles/shell.css";
import "./styles/analyzer.css";
import "./styles/mobile.css";
import "./styles/smartclear.css";

// The last window is drawn on the very first frame, from the copy saved while the app ran; the backend's answer takes over a moment later.
useStore.getState().bootFromCache();

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
