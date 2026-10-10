// Dialogs A's frame: a centred `.dialog` over the scrim, its head led by a kicker (who asks) or an
// icon tile (what is at stake), then the title, description, body and footer. The kit's `Dialog`
// has neither lead, nor the hint at the footer's left, so this is the kit's markup with them.

import type { KeyboardEvent, ReactNode } from "react";
import { Dialog as DialogPrimitive } from "radix-ui";

import { cn } from "@/lib/utils";

export function DialogFrame({
  title,
  description,
  lead,
  children,
  hint,
  actions,
  width,
  role = "dialog",
  onEscape,
  onKeyDown,
  autoFocus = true,
  tight = false,
}: {
  title: ReactNode;
  description?: ReactNode;
  /** Above the title: a `Kicker` or a `Tile`. */
  lead?: ReactNode;
  children?: ReactNode;
  /** At the footer's left, muted. */
  hint?: ReactNode;
  actions: ReactNode;
  width: number;
  role?: "dialog" | "alertdialog";
  /** What Escape does; without it Escape does nothing. Clicking the scrim never answers. */
  onEscape?: () => void;
  onKeyDown?: (event: KeyboardEvent<HTMLDivElement>) => void;
  /** Whether Radix focuses the first control; `false` focuses the dialog itself. */
  autoFocus?: boolean;
  /** Less room above the body, for a single subject card under the description. */
  tight?: boolean;
}) {
  return (
    <DialogPrimitive.Root open>
      <DialogPrimitive.Portal>
        <DialogPrimitive.Overlay className="scrim fixed">
          <DialogPrimitive.Content
            role={role}
            className="dialog outline-none"
            style={{ width }}
            onEscapeKeyDown={(event) => {
              event.preventDefault();
              onEscape?.();
            }}
            onPointerDownOutside={(event) => event.preventDefault()}
            onInteractOutside={(event) => event.preventDefault()}
            onOpenAutoFocus={(event) => {
              if (autoFocus) return;
              event.preventDefault();
              (event.currentTarget as HTMLElement | null)?.focus();
            }}
            onKeyDown={onKeyDown}
            tabIndex={-1}
          >
            <div className="dialog-head">
              {lead}
              <DialogPrimitive.Title className="dialog-title">{title}</DialogPrimitive.Title>
              {description ? (
                <DialogPrimitive.Description className="dialog-desc">{description}</DialogPrimitive.Description>
              ) : (
                <DialogPrimitive.Description className="sr-only">{title}</DialogPrimitive.Description>
              )}
            </div>
            {children && <div className={cn("dialog-body", tight && "dg-tight")}>{children}</div>}
            <div className="dialog-foot">
              {hint && <span className="dg-foot-hint">{hint}</span>}
              {actions}
            </div>
          </DialogPrimitive.Content>
        </DialogPrimitive.Overlay>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
  );
}

/** Who asks, above the title: "Browser request · just now". */
export function Kicker({ icon, children }: { icon: ReactNode; children: ReactNode[] }) {
  return (
    <div className="dg-kicker">
      {icon}
      {children.map((part, index) => (
        <span key={index} className="contents">
          {index > 0 && <span className="sep" />}
          <span>{part}</span>
        </span>
      ))}
    </div>
  );
}

/** What is at stake, above the title: a tinted icon. */
export function Tile({ tone, children }: { tone: "accent" | "warn" | "danger"; children: ReactNode }) {
  return <div className={cn("dg-tile", tone)}>{children}</div>;
}
