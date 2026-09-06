import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import { applyTheme, loadStoredThemeId } from "./themes";
import "./index.css";
import "./themes.css";

applyTheme(loadStoredThemeId());

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
