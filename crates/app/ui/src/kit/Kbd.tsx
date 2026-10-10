import { comboLabel } from "@/lib/shortcuts";
import { cn } from "@/lib/utils";

/** Key caps for a combo written as in DESIGN.md: `<Kbd combo="Ctrl K" />`. */
export function Kbd({ combo, className }: { combo: string; className?: string }) {
  const keys = comboLabel(combo);
  return (
    <span className={cn("kbds", className)}>
      {keys.map((key, index) => (
        <kbd key={index}>{key}</kbd>
      ))}
    </span>
  );
}
