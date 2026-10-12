// A row of the Entry detail's fields: label, value, its actions and copy button.

import type { ReactNode } from "react";
import { Check, Copy } from "lucide-react";

import type { CopyTarget } from "@/bindings";
import { cn } from "@/lib/utils";

/** Copies a field in Rust (`copyField`), flashing "Copied" on the row keyed `key`. */
export type CopyHandler = (field: CopyTarget, what: string, key: string) => Promise<void>;

/** A row of `.vb-fields`: label, value, and its copy button, flashing "Copied". */
export function FieldRow({
  label,
  copied,
  onCopy,
  actions,
  children,
}: {
  label: string;
  copied?: boolean;
  onCopy?: () => void;
  actions?: ReactNode;
  children: ReactNode;
}) {
  return (
    <div className={cn("vb-f", copied && "is-copied")}>
      <span className="k" title={label}>
        {label}
      </span>
      <span className="v">{children}</span>
      <span className="acts">
        {actions}
        {onCopy && (
          <button type="button" className="btn ghost sm icon" aria-label={`Copy ${label}`} title="Copy" onClick={onCopy}>
            <Copy />
          </button>
        )}
      </span>
      <span className="flash">
        <Check />
        Copied
      </span>
    </div>
  );
}
