import type { ReactNode } from "react";
import { ChevronDown, Check as CheckIcon } from "lucide-react";
import { Select as SelectPrimitive } from "radix-ui";

import { cn } from "@/lib/utils";

/** A select in the `.input` style, its options in a `.menu`. */
export function Select<Value extends string>({
  value,
  onValueChange,
  options,
  label,
  className,
}: {
  value: Value;
  onValueChange: (value: Value) => void;
  options: readonly { value: Value; label: ReactNode }[];
  label: string;
  className?: string;
}) {
  return (
    <SelectPrimitive.Root value={value} onValueChange={(next) => onValueChange(next as Value)}>
      <SelectPrimitive.Trigger aria-label={label} className={cn("input justify-between", className)}>
        <SelectPrimitive.Value />
        <SelectPrimitive.Icon className="text-fg-2">
          <ChevronDown className="ico-sm" />
        </SelectPrimitive.Icon>
      </SelectPrimitive.Trigger>
      <SelectPrimitive.Portal>
        <SelectPrimitive.Content position="popper" sideOffset={4} className="menu z-[60] min-w-[var(--radix-select-trigger-width)]">
          <SelectPrimitive.Viewport>
            {options.map((option) => (
              <SelectPrimitive.Item
                key={option.value}
                value={option.value}
                className="menu-item outline-none data-[highlighted]:bg-bg-active"
              >
                <SelectPrimitive.ItemText>{option.label}</SelectPrimitive.ItemText>
                <SelectPrimitive.ItemIndicator className="ml-auto">
                  <CheckIcon />
                </SelectPrimitive.ItemIndicator>
              </SelectPrimitive.Item>
            ))}
          </SelectPrimitive.Viewport>
        </SelectPrimitive.Content>
      </SelectPrimitive.Portal>
    </SelectPrimitive.Root>
  );
}
