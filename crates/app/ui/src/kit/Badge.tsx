import type { ComponentProps } from "react";

import type { Color } from "@/bindings";
import { cn } from "@/lib/utils";

export type BadgeTone = "neutral" | "accent" | "danger" | "warn" | "ok" | "info";

/** `.badge`, neutral or toned. */
export function Badge({ tone = "neutral", className, ...props }: ComponentProps<"span"> & { tone?: BadgeTone }) {
  return <span className={cn("badge", tone !== "neutral" && tone, className)} {...props} />;
}

/** The token of an Entry colour: the prototype calls violet purple. */
export function colorVar(color: Color): string {
  return `var(--c-${color === "violet" ? "purple" : color})`;
}

/** `.dot` in an Entry's colour. */
export function ColorDot({ color, className }: { color: Color; className?: string }) {
  return <span className={cn("dot", className)} style={{ background: colorVar(color) }} />;
}

/** `.tag`, with a coloured dot when given a colour. */
export function Tag({ color, className, children, ...props }: ComponentProps<"span"> & { color?: Color }) {
  return (
    <span className={cn("tag", className)} {...props}>
      {color && <ColorDot color={color} />}
      {children}
    </span>
  );
}
