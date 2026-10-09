import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "@/assets/globals.css";
import "./palette.css";
import { Palette } from "./Palette";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <Palette />
  </StrictMode>,
);
