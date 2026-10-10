import { Switch as SwitchPrimitive } from "radix-ui";

import { cn } from "@/lib/utils";

/** `.switch` for booleans. */
export function Switch({
  checked,
  onCheckedChange,
  label,
  disabled,
  className,
}: {
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
  label: string;
  disabled?: boolean;
  className?: string;
}) {
  return (
    <SwitchPrimitive.Root
      checked={checked}
      onCheckedChange={onCheckedChange}
      disabled={disabled}
      aria-label={label}
      className={cn("switch focus-visible:outline-none focus-visible:shadow-[var(--ring)]", checked && "is-on", className)}
    />
  );
}

/** `.check`, a checkbox, with its label. */
export function Check({
  checked,
  onCheckedChange,
  children,
  className,
}: {
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
  children?: React.ReactNode;
  className?: string;
}) {
  return (
    <label className={cn("hstack cursor-default", className)}>
      <button
        type="button"
        role="checkbox"
        aria-checked={checked}
        className={cn("check", checked && "is-on")}
        onClick={() => onCheckedChange(!checked)}
      />
      {children}
    </label>
  );
}
