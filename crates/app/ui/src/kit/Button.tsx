import { type ComponentProps, forwardRef } from "react";
import { cva, type VariantProps } from "class-variance-authority";

import { cn } from "@/lib/utils";

import { Kbd } from "./Kbd";

/** `.btn` and its variants from components.css. One `primary` per view. */
export const buttonVariants = cva("btn", {
  variants: {
    variant: {
      primary: "primary",
      secondary: "secondary",
      ghost: "ghost",
      danger: "danger",
      dangerGhost: "danger-ghost",
      plain: "",
    },
    size: { md: "", sm: "sm", lg: "lg" },
  },
  defaultVariants: { variant: "secondary", size: "md" },
});

export interface ButtonProps extends ComponentProps<"button">, VariantProps<typeof buttonVariants> {
  /** A trailing shortcut hint, such as "Ctrl S" — DESIGN.md encourages them. */
  shortcut?: string;
}

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(function Button(
  { className, variant, size, shortcut, children, type = "button", ...props },
  ref,
) {
  return (
    <button ref={ref} type={type} className={cn(buttonVariants({ variant, size }), className)} {...props}>
      {children}
      {shortcut && <Kbd combo={shortcut} />}
    </button>
  );
});

export interface IconButtonProps extends ComponentProps<"button">, VariantProps<typeof buttonVariants> {
  /** What the button does, for screen readers and its tooltip. */
  label: string;
}

/** `.btn.icon`: a square button with just an icon. */
export const IconButton = forwardRef<HTMLButtonElement, IconButtonProps>(function IconButton(
  { className, variant = "ghost", size, label, type = "button", ...props },
  ref,
) {
  return (
    <button
      ref={ref}
      type={type}
      aria-label={label}
      title={label}
      className={cn(buttonVariants({ variant, size }), "icon", className)}
      {...props}
    />
  );
});
