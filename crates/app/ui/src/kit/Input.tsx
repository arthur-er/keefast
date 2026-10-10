import { type ComponentProps, forwardRef, type ReactNode, useState } from "react";
import { Eye, EyeOff } from "lucide-react";

import { cn } from "@/lib/utils";

export interface InputProps extends ComponentProps<"input"> {
  mono?: boolean;
  invalid?: boolean;
  inputSize?: "md" | "lg";
}

/** `.input`: a 32px text field; `mono` for secrets, `invalid` after a refused value. */
export const Input = forwardRef<HTMLInputElement, InputProps>(function Input(
  { className, mono, invalid, inputSize = "md", ...props },
  ref,
) {
  return (
    <input
      ref={ref}
      className={cn("input", mono && "mono", invalid && "invalid", inputSize === "lg" && "lg", className)}
      aria-invalid={invalid || undefined}
      {...props}
    />
  );
});

export interface AffixInputProps extends InputProps {
  /** Before the text: an icon. */
  leading?: ReactNode;
  /** After the text: buttons, such as reveal or generate. */
  trailing?: ReactNode;
}

/** `.input` as a box around an input, with icons or buttons inside, as the prototype's fields. */
export const AffixInput = forwardRef<HTMLInputElement, AffixInputProps>(function AffixInput(
  { className, mono, invalid, inputSize = "md", leading, trailing, ...props },
  ref,
) {
  return (
    <label className={cn("input", mono && "mono", invalid && "invalid", inputSize === "lg" && "lg", className)}>
      {leading}
      <input ref={ref} aria-invalid={invalid || undefined} {...props} />
      {trailing}
    </label>
  );
});

/** A masked secret with an eye to reveal it, in the mono face. */
export const PasswordInput = forwardRef<HTMLInputElement, AffixInputProps & { revealLabel?: string }>(
  function PasswordInput({ trailing, revealLabel = "Show", ...props }, ref) {
    const [shown, setShown] = useState(false);
    return (
      <AffixInput
        ref={ref}
        mono
        type={shown ? "text" : "password"}
        autoComplete="off"
        spellCheck={false}
        trailing={
          <>
            {trailing}
            <button
              type="button"
              className="btn ghost sm icon"
              aria-label={shown ? "Hide" : revealLabel}
              title={shown ? "Hide" : revealLabel}
              onClick={() => setShown((value) => !value)}
            >
              {shown ? <EyeOff /> : <Eye />}
            </button>
          </>
        }
        {...props}
      />
    );
  },
);

/** `textarea.input`. */
export const Textarea = forwardRef<HTMLTextAreaElement, ComponentProps<"textarea"> & { invalid?: boolean }>(
  function Textarea({ className, invalid, ...props }, ref) {
    return <textarea ref={ref} className={cn("input", invalid && "invalid", className)} {...props} />;
  },
);

/** `.field`: a label over a control, with a hint or error under it. */
export function Field({
  label,
  hint,
  error,
  htmlFor,
  children,
  className,
}: {
  label: ReactNode;
  hint?: ReactNode;
  error?: ReactNode;
  htmlFor?: string;
  children: ReactNode;
  className?: string;
}) {
  return (
    <div className={cn("field", className)}>
      <label htmlFor={htmlFor}>{label}</label>
      {children}
      {error ? <div className="error-msg">{error}</div> : hint ? <div className="hint">{hint}</div> : null}
    </div>
  );
}
