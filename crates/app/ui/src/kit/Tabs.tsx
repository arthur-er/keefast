import type { ReactNode } from "react";
import { Tabs as TabsPrimitive } from "radix-ui";

import { cn } from "@/lib/utils";

/** Tabs as the Vault browser C prototype draws them (`.C-tabs`): underlined, with counts. */
export const Tabs = TabsPrimitive.Root;
export const TabsContent = TabsPrimitive.Content;

export function TabsList({ className, children }: { className?: string; children: ReactNode }) {
  return (
    <TabsPrimitive.List className={cn("flex gap-0.5 border-b border-border", className)}>{children}</TabsPrimitive.List>
  );
}

export function TabsTrigger({ value, count, children }: { value: string; count?: number; children: ReactNode }) {
  return (
    <TabsPrimitive.Trigger
      value={value}
      className={cn(
        "relative inline-flex h-[34px] items-center gap-1.5 px-2.5 text-md font-medium text-fg-2 outline-none hover:text-fg-0",
        "data-[state=active]:text-fg-0 focus-visible:shadow-[var(--ring)] rounded-xs",
        "data-[state=active]:after:absolute data-[state=active]:after:inset-x-2 data-[state=active]:after:-bottom-px data-[state=active]:after:h-0.5 data-[state=active]:after:rounded-xs data-[state=active]:after:bg-fg-0 data-[state=active]:after:content-['']",
      )}
    >
      {children}
      {count ? <span className="text-xs font-normal text-fg-2">{count}</span> : null}
    </TabsPrimitive.Trigger>
  );
}
