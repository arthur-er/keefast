import type { ReactNode } from "react";
import { Dialog as DialogPrimitive } from "radix-ui";

import { cn } from "@/lib/utils";

/** A centred `.dialog` over a `.scrim`, for decisions that block (DESIGN.md §1.6): unlock,
 * confirm delete, a browser asking for access. Header = title + one-line description; footer =
 * right-aligned actions, primary last. */
export function Dialog({
  open,
  onOpenChange,
  title,
  description,
  children,
  footer,
  className,
  dismissible = true,
}: {
  open: boolean;
  onOpenChange?: (open: boolean) => void;
  title: ReactNode;
  description?: ReactNode;
  children?: ReactNode;
  footer?: ReactNode;
  className?: string;
  /** Whether Escape and clicking the scrim close it. Not for answers the app must have. */
  dismissible?: boolean;
}) {
  return (
    <DialogPrimitive.Root open={open} onOpenChange={onOpenChange}>
      <DialogPrimitive.Portal>
        <DialogPrimitive.Overlay className="scrim fixed">
          <DialogPrimitive.Content
            className={cn("dialog outline-none", className)}
            onEscapeKeyDown={(event) => dismissible || event.preventDefault()}
            onPointerDownOutside={(event) => dismissible || event.preventDefault()}
            onInteractOutside={(event) => dismissible || event.preventDefault()}
            aria-describedby={description ? undefined : undefined}
          >
            <div className="dialog-head">
              <DialogPrimitive.Title className="dialog-title">{title}</DialogPrimitive.Title>
              {description ? (
                <DialogPrimitive.Description className="dialog-desc">{description}</DialogPrimitive.Description>
              ) : (
                <DialogPrimitive.Description className="sr-only">{title}</DialogPrimitive.Description>
              )}
            </div>
            {children && <div className="dialog-body">{children}</div>}
            {footer && <div className="dialog-foot">{footer}</div>}
          </DialogPrimitive.Content>
        </DialogPrimitive.Overlay>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
  );
}

export const DialogClose = DialogPrimitive.Close;
