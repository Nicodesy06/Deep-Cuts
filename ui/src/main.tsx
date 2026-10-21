import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import { runSmokeIfRequested } from "./smoke";
import "./styles.css";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);

void runSmokeIfRequested();
