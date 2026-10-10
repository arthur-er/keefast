// TOTP B: the setup that expands inline under the TOTP field, in the Entry's detail and in Entry
// editor A. A secret key with its settings, or an otpauth:// URI that brings its own; the code
// they make shows live before saving. Saving and removing are the caller's: at once on a saved
// Entry, or with the Entry for a new one.

import { useEffect, useRef, useState } from "react";
import { CircleAlert, ClipboardPaste, Link, Timer, Trash2, X } from "lucide-react";

import { api, errorMessage } from "@/api";
import type { TotpCodeView, TotpSetup as Setup } from "@/bindings";
import { useTotpCode } from "@/api/queries";
import { Button, IconButton, Kbd, Segmented, toast } from "@/kit";
import { cn } from "@/lib/utils";

import { ALGORITHMS, EMPTY_SETUP, errorField, splitCode, uriSettings } from "./totp";
import "./totp.css";

export function TotpSetupPanel({
  title,
  load,
  initial,
  onSave,
  onRemove,
  onClose,
  removeLabel = "Remove TOTP",
  note = "Saved with the Entry, encrypted like its password.",
}: {
  /** "Set up TOTP", or "TOTP settings" for an Entry that has TOTP. */
  title: string;
  /** Reads the settings to start from (the Entry's, secret included), if it has any. */
  load?: () => Promise<Setup | null>;
  initial?: Setup | null;
  /** Saves the settings; a thrown error shows by the control it is about. */
  onSave: (setup: Setup) => Promise<void>;
  /** Shown when the Entry has TOTP. */
  onRemove?: () => Promise<void>;
  onClose: () => void;
  removeLabel?: string;
  note?: string;
}) {
  const [setup, setSetup] = useState<Setup>(initial ?? EMPTY_SETUP);
  const [loaded, setLoaded] = useState(!load);
  const [tried, setTried] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const secretField = useRef<HTMLInputElement>(null);
  const touched = useRef(false);

  useEffect(() => {
    if (!load) return void secretField.current?.focus();
    let live = true;
    load()
      // What the user typed meanwhile wins.
      .then((found) => live && found && !touched.current && setSetup(found))
      .catch((error) => live && toast(errorMessage(error), { icon: "error" }))
      .finally(() => {
        if (!live) return;
        setLoaded(true);
        requestAnimationFrame(() => secretField.current?.focus());
      });
    return () => {
      live = false;
    };
    // The panel loads once, as it opens.
  }, []);

  const preview = usePreview(setup, loaded);
  const fromUri = uriSettings(setup.secretOrUri);
  const error = saveError ?? (tried && preview.error) ?? null;
  const errorAt = error ? errorField(error) : null;
  const change = (next: Partial<Setup>) => {
    touched.current = true;
    setSetup((current) => ({ ...current, ...next }));
    setSaveError(null);
  };

  const save = async () => {
    setTried(true);
    if (busy) return;
    setBusy(true);
    try {
      await onSave(setup);
    } catch (thrown) {
      setSaveError(errorMessage(thrown));
    } finally {
      setBusy(false);
    }
  };
  const paste = async () => {
    try {
      const text = await navigator.clipboard.readText();
      if (text.trim()) change({ secretOrUri: text.trim() });
      secretField.current?.focus();
    } catch {
      toast("keefast can't read the clipboard here: paste with Ctrl V.", { icon: "info" });
      secretField.current?.focus();
    }
  };

  const errorLine = (at: "secret" | "digits" | "period") =>
    errorAt === at && (
      <div className="error-msg" role="alert">
        <CircleAlert />
        {error}
      </div>
    );

  return (
    <div
      className="ix"
      role="group"
      aria-label={title}
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          event.preventDefault();
          event.stopPropagation();
          onClose();
        } else if (event.key === "Enter" && !(event.target instanceof HTMLButtonElement)) {
          event.preventDefault();
          void save();
        }
      }}
    >
      <div className="ix-head">
        <h3>{title}</h3>
        <span className="spacer" />
        <IconButton size="sm" label="Close (Esc)" onClick={onClose}>
          <X />
        </IconButton>
      </div>
      <div className="ix-body">
        <div className="tf">
          <div className="field">
            <label htmlFor="tt-secret">Secret key or otpauth:// URI</label>
            <div className={cn("input mono", errorAt === "secret" && "invalid")}>
              <input
                id="tt-secret"
                ref={secretField}
                value={setup.secretOrUri}
                placeholder={loaded ? "e.g. JBSWY3DPEHPK3PXP" : "…"}
                spellCheck={false}
                autoComplete="off"
                aria-invalid={errorAt === "secret" || undefined}
                onChange={(event) => change({ secretOrUri: event.target.value })}
              />
              <Button variant="ghost" size="sm" className="-mr-1.5" onClick={() => void paste()}>
                <ClipboardPaste /> Paste
              </Button>
            </div>
            {errorLine("secret") ||
              (fromUri ? (
                <div className="tf-uri">
                  <Link />
                  <span>
                    From the URI: <b>{fromUri.issuer ?? "Unknown issuer"}</b>
                    {fromUri.account && ` · ${fromUri.account}`}. Digits, period and algorithm come from it too.
                  </span>
                </div>
              ) : (
                <div className="hint">Paste the otpauth:// URI from the site's QR code, or type its secret key and settings.</div>
              ))}
          </div>
          <div className="tf-grid">
            <div className="field">
              <label htmlFor="tt-digits">Digits</label>
              <div className={cn("input mono", errorAt === "digits" && "invalid", fromUri && "is-off")}>
                <input
                  id="tt-digits"
                  inputMode="numeric"
                  value={fromUri ? (fromUri.digits ?? "6") : setup.digits}
                  disabled={!!fromUri}
                  onChange={(event) => change({ digits: event.target.value })}
                />
              </div>
            </div>
            <div className="field">
              <label htmlFor="tt-period">Period (seconds)</label>
              <div className={cn("input mono", errorAt === "period" && "invalid", fromUri && "is-off")}>
                <input
                  id="tt-period"
                  inputMode="numeric"
                  value={fromUri ? (fromUri.period ?? "30") : setup.period}
                  disabled={!!fromUri}
                  onChange={(event) => change({ period: event.target.value })}
                />
              </div>
            </div>
            <div className="field">
              <label>Algorithm</label>
              <Segmented
                label="Algorithm"
                className={cn(fromUri && "is-off")}
                value={fromUri ? (fromUri.algorithm ?? "sha1") : setup.algorithm}
                onValueChange={(algorithm) => change({ algorithm })}
                options={ALGORITHMS}
              />
            </div>
          </div>
          {errorLine("digits")}
          {errorLine("period")}
          <Preview code={preview.code} />
        </div>
      </div>
      <div className="ix-foot">
        {onRemove ? (
          <Button
            variant="dangerGhost"
            size="sm"
            disabled={busy}
            onClick={async () => {
              setBusy(true);
              try {
                await onRemove();
              } catch (thrown) {
                toast(errorMessage(thrown), { icon: "error" });
              } finally {
                setBusy(false);
              }
            }}
          >
            <Trash2 /> {removeLabel}
          </Button>
        ) : (
          <span className="hint">{note}</span>
        )}
        <span className="spacer" />
        <Button variant="ghost" size="sm" onClick={onClose}>
          Cancel <Kbd combo="Escape" />
        </Button>
        <Button variant="primary" size="sm" disabled={busy || !loaded} onClick={() => void save()}>
          {onRemove ? "Save" : "Set up TOTP"} <Kbd combo="Enter" />
        </Button>
      </div>
    </div>
  );
}

/** The code the settings make now, asked of the backend as they change and as each code runs
 * out. */
function usePreview(setup: Setup, enabled: boolean): { code: TotpCodeView | null; error: string | null } {
  const [state, setState] = useState<{ code: TotpCodeView | null; error: string | null }>({ code: null, error: null });
  const [tick, setTick] = useState(0);
  const key = JSON.stringify(setup);
  useEffect(() => {
    if (!enabled) return;
    let live = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const delay = setTimeout(() => {
      api
        .previewTotp(setup)
        .then((code) => {
          if (!live) return;
          setState({ code, error: null });
          timer = setTimeout(() => setTick((n) => n + 1), code.remaining * 1000);
        })
        .catch((error) => live && setState({ code: null, error: errorMessage(error) }));
    }, 120);
    return () => {
      live = false;
      clearTimeout(delay);
      clearTimeout(timer);
    };
    // `key` stands for the setup.
  }, [key, enabled, tick]);
  return state;
}

function Preview({ code }: { code: TotpCodeView | null }) {
  const left = useCountdown(code);
  if (!code) {
    return (
      <div className="tt-prev is-empty">
        <Timer />
        <span>The current code shows here once the secret key is valid.</span>
      </div>
    );
  }
  const [a, b] = splitCode(code.code);
  return (
    <div className={cn("tt-prev", left <= 5 && "is-low")}>
      <span className="lbl">Code now</span>
      <span className="tt-code">
        <span>{a}</span>
        <span>{b}</span>
      </span>
      <span className="spacer" />
      <span className="ring" style={{ "--p": left / code.period } as React.CSSProperties} />
      <span className="tt-secs">{left} s</span>
    </div>
  );
}

/** Seconds left of `code`, counting down each second. */
function useCountdown(code: TotpCodeView | null): number {
  const [left, setLeft] = useState(code?.remaining ?? 0);
  useEffect(() => {
    setLeft(code?.remaining ?? 0);
    if (!code) return;
    const timer = setInterval(() => setLeft((value) => Math.max(0, value - 1)), 1000);
    return () => clearInterval(timer);
  }, [code]);
  return left;
}

/** The Entry's TOTP code now, its countdown ring and seconds: TOTP as a field. */
export function LiveTotp({ vault, entry, className }: { vault: number; entry: string; className?: string }) {
  const totp = useTotpCode(vault, entry);
  const code = totp.data ?? null;
  const left = useCountdown(code);
  if (!code) return <span className={cn("tt-live subtle", className)}>…</span>;
  const [a, b] = splitCode(code.code);
  return (
    <span className={cn("tt-live", left <= 5 && "is-low", className)}>
      <span className="ring" style={{ "--p": left / code.period } as React.CSSProperties} />
      <span className="tt-code">
        <span>{a}</span>
        <span>{b}</span>
      </span>
      <span className="tt-secs">{left} s</span>
    </span>
  );
}
