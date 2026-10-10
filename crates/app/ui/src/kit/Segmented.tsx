import type { ReactNode } from "react";

import { cn } from "@/lib/utils";

/** `.seg`: 2–4 exclusive options, such as the theme or the generator's mode. */
export function Segmented<Value extends string>({
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
    <div className={cn("seg", className)} role="radiogroup" aria-label={label}>
      {options.map((option) => (
        <button
          key={option.value}
          type="button"
          role="radio"
          aria-checked={option.value === value}
          className={cn(option.value === value && "is-on")}
          onClick={() => onValueChange(option.value)}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}
