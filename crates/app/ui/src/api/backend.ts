// The backend the UI talks to: the Tauri app through the generated bindings, or the mock
// (./mock) in a normal browser. Both have exactly this shape, so the mock is type-checked
// against every command and event the Rust side exposes.

import type { commands, events } from "@/bindings";

/** Every command, as generated: each resolves to `{ status: "ok", data }` or
 * `{ status: "error", error: ApiError }`. */
export type Commands = typeof commands;

type PayloadOf<E> = E extends { listen: (callback: (event: infer Event) => unknown) => unknown }
  ? Event extends { payload: infer Payload }
    ? Payload
    : never
  : never;

/** The payload of each event. */
export type EventPayloads = { [Name in keyof typeof events]: PayloadOf<(typeof events)[Name]> };

/** Every event, to listen to. `listen` resolves to a function that stops listening. */
export type Events = {
  [Name in keyof EventPayloads]: {
    listen(callback: (event: { payload: EventPayloads[Name] }) => void): Promise<() => void>;
  };
};

export interface Backend {
  kind: "tauri" | "mock";
  commands: Commands;
  events: Events;
}
