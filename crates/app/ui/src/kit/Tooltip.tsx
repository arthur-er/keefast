import type { ReactNode } from "react";
import { Tooltip as TooltipPrimitive } from "radix-ui";

import { Kbd } from "./Kbd";

export const TooltipProvider = TooltipPrimitive.Provider;

/** `.tooltip`: a label, and the shortcut when there is one. */
export function Tooltip({
  content,
  shortcut,
  side = "bottom",
  children,
}: {
  content: ReactNode;
  shortcut?: string;
  side?: "top" | "bottom" | "left" | "right";
  children: ReactNode;
}) {
  return (
    <TooltipPrimitive.Root delayDuration={500}>
      <TooltipPrimitive.Trigger asChild>{children}</TooltipPrimitive.Trigger>
      <TooltipPrimitive.Portal>
        <TooltipPrimitive.Content side={side} sideOffset={6} className="tooltip z-[70] animate-kf-fade">
          {content}
          {shortcut && <Kbd combo={shortcut} />}
        </TooltipPrimitive.Content>
      </TooltipPrimitive.Portal>
    </TooltipPrimitive.Root>
  );
}
