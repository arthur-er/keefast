// The typed API the screens use: the backend's commands with their results unwrapped (a failed
// command throws an ApiError), its events, and which backend is in use.
//
// The mock backend is used in `pnpm dev:mock` (Vite's `mock` mode) and when the dev server is
// opened in a normal browser rather than the Tauri window. Production builds never include it.

import type { ApiError as ApiErrorBody, ErrorKind } from "@/bindings";

import type { Backend, Commands, EventPayloads } from "./backend";
import { tauriBackend } from "./tauri";

export type { EventPayloads } from "./backend";
export type * from "@/bindings";

/** Whether the page runs in keefast's Tauri window. */
export const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function chooseBackend(): Promise<Backend> {
  // Written so a production build drops the mock entirely: both conditions are constants there.
  if (import.meta.env.DEV || import.meta.env.MODE === "mock") {
    if (import.meta.env.MODE === "mock" || !inTauri) {
      return (await import("./mock")).createMockBackend();
    }
  }
  return tauriBackend;
}

export const backend: Backend = await chooseBackend();

if (backend.kind === "mock") {
  console.info("keefast: mock backend (fake Vaults; any master password but \"wrong\" unlocks)");
}

/** A failed command: `kind` to branch on, `message` to show, `detail` for the curious. */
export class ApiError extends Error {
  readonly kind: ErrorKind;
  readonly detail: string | null;

  constructor(body: ApiErrorBody) {
    super(body.message);
    this.name = "ApiError";
    this.kind = body.kind;
    this.detail = body.detail;
  }
}

type Unwrapped<Command> = Command extends (
  ...args: infer Args
) => Promise<{ status: "ok"; data: infer Data } | { status: "error"; error: unknown }>
  ? (...args: Args) => Promise<Data>
  : never;

/** Every command, resolving to its data or throwing an `ApiError`. */
export type Api = { [Name in keyof Commands]: Unwrapped<Commands[Name]> };

export function unwrap(commands: Commands): Api {
  const api: Record<string, unknown> = {};
  for (const [name, command] of Object.entries(commands)) {
    api[name] = async (...args: unknown[]) => {
      const result = await (command as (...a: unknown[]) => Promise<unknown>)(...args);
      const { status, data, error } = result as {
        status: "ok" | "error";
        data?: unknown;
        error?: ApiErrorBody;
      };
      if (status === "error") throw new ApiError(error!);
      return data;
    };
  }
  return api as Api;
}

export const api: Api = unwrap(backend.commands);

/** Listens to a backend event; returns a function that stops listening. */
export function onEvent<Name extends keyof EventPayloads>(
  name: Name,
  callback: (payload: EventPayloads[Name]) => void,
): () => void {
  let stopped = false;
  let stop: (() => void) | undefined;
  void backend.events[name]
    .listen((event) => callback(event.payload as EventPayloads[Name]))
    .then((unlisten) => {
      if (stopped) unlisten();
      else stop = unlisten;
    });
  return () => {
    stopped = true;
    stop?.();
  };
}

/** The message to show for anything a command threw. */
export function errorMessage(error: unknown): string {
  if (error instanceof ApiError) return error.message;
  if (error instanceof Error) return error.message;
  return String(error);
}
