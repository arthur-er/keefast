// `.toast`: one at a time, bottom centre, with an optional countdown bar (clipboard clearing).

import { type ReactNode, useEffect, useSyncExternalStore } from "react";
import { CircleAlert, Check as CheckIcon, ClipboardCheck, Info } from "lucide-react";

export interface ToastOptions {
  /** A countdown, as for "clears in 30s"; the bar runs down over it. */
  seconds?: number;
  icon?: "check" | "clipboard" | "info" | "error";
  /** How long it stays up, in ms; default 2.4 s, or up to 4 s for a countdown. */
  duration?: number;
}

interface ToastState extends ToastOptions {
  id: number;
  message: ReactNode;
}

let current: ToastState | null = null;
let nextId = 1;
const subscribers = new Set<() => void>();
const notify = () => subscribers.forEach((subscriber) => subscriber());

/** Shows a toast, replacing the one up. Returns a function that hides it. */
export function toast(message: ReactNode, options: ToastOptions = {}): () => void {
  const id = nextId++;
  current = { id, message, ...options };
  notify();
  return () => dismissToast(id);
}

export function dismissToast(id?: number) {
  if (id === undefined || current?.id === id) {
    current = null;
    notify();
  }
}

function useToast() {
  return useSyncExternalStore(
    (subscriber) => {
      subscribers.add(subscriber);
      return () => subscribers.delete(subscriber);
    },
    () => current,
  );
}

const ICONS = { check: CheckIcon, clipboard: ClipboardCheck, info: Info, error: CircleAlert };

/** Where toasts show. Mount once. */
export function Toaster() {
  const shown = useToast();
  useEffect(() => {
    if (!shown) return;
    const duration = shown.duration ?? (shown.seconds ? Math.min(shown.seconds, 4) * 1000 : 2400);
    const timer = setTimeout(() => dismissToast(shown.id), duration);
    return () => clearTimeout(timer);
  }, [shown]);
  if (!shown) return null;
  const Icon = ICONS[shown.icon ?? "check"];
  return (
    <div className="toast-host" role="status" aria-live="polite">
      <div className="toast" key={shown.id}>
        <Icon className={shown.icon === "error" ? "text-danger" : undefined} />
        <span>{shown.message}</span>
        {shown.seconds ? (
          <>
            <span className="subtle mono">{shown.seconds}s</span>
            <div className="toast-bar" style={{ animationDuration: `${shown.seconds}s` }} />
          </>
        ) : null}
      </div>
    </div>
  );
}
