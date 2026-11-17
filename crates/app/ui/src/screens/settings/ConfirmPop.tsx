// The small popover a settings row opens above its buttons, such as "Revoke the Agent?":
// Escape closes it and leaves the page under it open.

import type { ReactNode } from "react";

import { Button, PopoverContent } from "@/kit";

/** A row's popover, above its buttons; Escape closes only the popover. */
export function RowPop({ onClose, children }: { onClose: () => void; children: ReactNode }): ReactNode {
  return (
    <PopoverContent
      side="top"
      align="end"
      className="confirm-pop"
      onEscapeKeyDown={(event) => {
        event.preventDefault();
        onClose();
      }}
    >
      {children}
    </PopoverContent>
  );
}

/** Asks before something that can't be undone; Cancel closes, the danger button does it. */
export function ConfirmPop({
  title,
  action,
  autoFocus = true,
  onCancel,
  onConfirm,
  children,
}: {
  title: ReactNode;
  /** The danger button's label, such as "Revoke". */
  action: string;
  autoFocus?: boolean;
  onCancel: () => void;
  onConfirm: () => void;
  /** What happens, in a sentence or two. */
  children: ReactNode;
}): ReactNode {
  return (
    <RowPop onClose={onCancel}>
      <h4>{title}</h4>
      <p>{children}</p>
      <div className="hstack justify-end">
        <Button size="sm" variant="ghost" onClick={onCancel}>
          Cancel
        </Button>
        <Button size="sm" variant="danger" autoFocus={autoFocus} onClick={onConfirm}>
          {action}
        </Button>
      </div>
    </RowPop>
  );
}
