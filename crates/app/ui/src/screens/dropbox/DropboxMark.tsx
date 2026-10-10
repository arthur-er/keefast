import { cn } from "@/lib/utils";

/** Dropbox's mark on its blue tile, as the prototype draws it. */
export function DropboxMark({ size, className }: { size?: "md" | "lg"; className?: string }) {
  return (
    <span className={cn("dbx-mark", size, className)} aria-hidden>
      <svg viewBox="0 0 24 24" fill="currentColor">
        <path d="M6 1.807 0 5.629l6 3.822 6.001-3.822L6 1.807zm12 0-6 3.822 6 3.822 6-3.822-6-3.822zM0 13.274l6 3.822 6.001-3.822L6 9.452l-6 3.822zm18-3.822-6 3.822 6 3.822 6-3.822-6-3.822zM6 18.371l6.001 3.822 6-3.822-6-3.822L6 18.371z" />
      </svg>
    </span>
  );
}
