import "./index.css";

import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { markStartup } from "@/shared/lib/startupTiming";

import { App } from "./App";

markStartup("frontend_module_loaded");

const root = createRoot(document.getElementById("root")!);
markStartup("react_root_created");

root.render(
  <StrictMode>
    <App />
  </StrictMode>,
);
markStartup("react_render_scheduled");
