// Checked by `pnpm typecheck`: an id no command has doesn't compile.

import { describe, expectTypeOf, it } from "vitest";

import type { ShortcutName } from "@/lib/shortcuts";

import type { AppCommandId } from "./commands";

describe("command ids", () => {
  it("are a shortcut's command, another of the app's, or a Plugin's", () => {
    expectTypeOf<"lockAll">().toExtend<AppCommandId>();
    expectTypeOf<"saveAll">().toExtend<AppCommandId>();
    expectTypeOf<"plugin:keefast.card:card:copy-number">().toExtend<AppCommandId>();
    expectTypeOf<"flyToTheMoon">().not.toExtend<AppCommandId>();
  });

  it("are named after the shortcut a command runs, but for the keys commands share", () => {
    expectTypeOf<Exclude<ShortcutName, AppCommandId>>().toEqualTypeOf<"palette" | "close" | "edit" | "save" | "history">();
  });
});
