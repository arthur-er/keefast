import type { ComponentProps, ReactNode } from "react";
import { ContextMenu as ContextPrimitive, DropdownMenu as MenuPrimitive } from "radix-ui";

import { cn } from "@/lib/utils";

import { Kbd } from "./Kbd";

/** A dropdown `.menu` anchored to its trigger. */
export const Menu = MenuPrimitive.Root;
export const MenuTrigger = MenuPrimitive.Trigger;

export function MenuContent({ className, children, ...props }: ComponentProps<typeof MenuPrimitive.Content>) {
  return (
    <MenuPrimitive.Portal>
      <MenuPrimitive.Content sideOffset={4} align="start" className={cn("menu z-[60] outline-none", className)} {...props}>
        {children}
      </MenuPrimitive.Content>
    </MenuPrimitive.Portal>
  );
}

interface ItemProps {
  icon?: ReactNode;
  shortcut?: string;
  danger?: boolean;
  onSelect?: (event: Event) => void;
  disabled?: boolean;
  children: ReactNode;
}

const itemClass = (danger?: boolean) =>
  cn("menu-item outline-none data-[highlighted]:bg-bg-active data-[disabled]:opacity-45", danger && "danger");

/** `.menu-item`: icon, label, trailing shortcut. */
export function MenuItem({ icon, shortcut, danger, children, ...props }: ItemProps) {
  return (
    <MenuPrimitive.Item className={itemClass(danger)} {...props}>
      {icon}
      <span className="truncate">{children}</span>
      {shortcut && <Kbd combo={shortcut} />}
    </MenuPrimitive.Item>
  );
}

export const MenuSeparator = () => <MenuPrimitive.Separator className="menu-sep" />;
export const MenuLabel = ({ children }: { children: ReactNode }) => (
  <MenuPrimitive.Label className="menu-label">{children}</MenuPrimitive.Label>
);

/** A right-click `.menu`, as on Entry rows. */
export const ContextMenu = ContextPrimitive.Root;
export const ContextMenuTrigger = ContextPrimitive.Trigger;

export function ContextMenuContent({ className, children, ...props }: ComponentProps<typeof ContextPrimitive.Content>) {
  return (
    <ContextPrimitive.Portal>
      <ContextPrimitive.Content className={cn("menu z-[60] outline-none", className)} {...props}>
        {children}
      </ContextPrimitive.Content>
    </ContextPrimitive.Portal>
  );
}

export function ContextMenuItem({ icon, shortcut, danger, children, ...props }: ItemProps) {
  return (
    <ContextPrimitive.Item className={itemClass(danger)} {...props}>
      {icon}
      <span className="truncate">{children}</span>
      {shortcut && <Kbd combo={shortcut} />}
    </ContextPrimitive.Item>
  );
}

export const ContextMenuSeparator = () => <ContextPrimitive.Separator className="menu-sep" />;
