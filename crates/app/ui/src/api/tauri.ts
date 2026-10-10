import { commands, events } from "@/bindings";

import type { Backend } from "./backend";

export const tauriBackend: Backend = { kind: "tauri", commands, events };
