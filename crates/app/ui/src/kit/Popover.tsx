import type { ComponentProps } from "react";
import { Popover as PopoverPrimitive } from "radix-ui";

import { cn } from "@/lib/utils";

/** A `.popover` anchored to its trigger, scaling from it, as the generator on a password field. */
export const Popover = PopoverPrimitive.Root;
export const PopoverTrigger = PopoverPrimitive.Trigger;
export const PopoverAnchor = PopoverPrimitive.Anchor;
export const PopoverClose = PopoverPrimitive.Close;

export function PopoverContent({ className, ...props }: ComponentProps<typeof PopoverPrimitive.Content>) {
  return (
    <PopoverPrimitive.Portal>
      <PopoverPrimitive.Content
        sideOffset={6}
        align="start"
        className={cn("popover z-[60] outline-none origin-[var(--radix-popover-content-transform-origin)]", className)}
        {...props}
      />
    </PopoverPrimitive.Portal>
  );
}
