import React from "react";
import ReactDOM from "react-dom/client";

// Bundled font (no remote font service): Inter, the closest to SF Pro.
import "@fontsource-variable/inter";
import "./styles/index.css";

import App from "./App";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
