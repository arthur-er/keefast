// Password generator A: a popover on the password field. Characters or a passphrase, with the
// settings the app config remembers (every change is saved, as the GPUI generator did), a
// preview coloured by kind of character with the recipe's strength, "Use password" to fill the
// field, and "Settings…" for the defaults in General Settings.

import { type ReactNode, useEffect, useRef, useState } from "react";
import { CaseSensitive, CircleAlert, RefreshCw, SlidersHorizontal, WholeWord } from "lucide-react";
import { useQuery } from "@tanstack/react-query";

import { api, errorMessage } from "@/api";
import type { GeneratedPassword, GeneratorOptions } from "@/bindings";
import { keys, useSettings } from "@/api/queries";
import { Button, Kbd, Popover, PopoverAnchor, PopoverContent, PopoverTrigger, Segmented, StrengthMeter, Switch } from "@/kit";
import { isTyping } from "@/lib/shortcuts";
import { cn } from "@/lib/utils";

import { SEPARATORS, segments, sliderRange, toggleClass } from "./generator";
import "./password-generator.css";

const FALLBACK: GeneratorOptions = {
  mode: "characters",
  characters: { length: 20, lowercase: true, uppercase: true, digits: true, symbols: true, excludeLookalikes: false },
  passphrase: { words: 6, separator: "-" },
};

const CLASSES = [
  ["lowercase", "a–z"],
  ["uppercase", "A–Z"],
  ["digits", "0–9"],
  ["symbols", "!@#$"],
] as const;

/** The password field (`children`, holding a `GeneratorTrigger`) with the popover under it.
 * `onOpenSettings` is its "Settings…" link, to General Settings' password generator section. */
export function GeneratorPopover({
  open,
  onOpenChange,
  onUse,
  onOpenSettings,
  children,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onUse: (password: string) => void;
  onOpenSettings?: () => void;
  children: ReactNode;
}) {
  return (
    <Popover open={open} onOpenChange={onOpenChange}>
      <PopoverAnchor asChild>{children}</PopoverAnchor>
      {open && (
        <PopoverContent
          align="end"
          className="gp"
          aria-label="Generate a password"
          onOpenAutoFocus={(event) => event.preventDefault()}
        >
          <Generator
            onUse={(password) => {
              onUse(password);
              onOpenChange(false);
            }}
            onOpenSettings={
              onOpenSettings &&
              (() => {
                onOpenChange(false);
                onOpenSettings();
              })
            }
          />
        </PopoverContent>
      )}
    </Popover>
  );
}

/** What opens the generator: a button inside the field. */
export const GeneratorTrigger = PopoverTrigger;

function Generator({ onUse, onOpenSettings }: { onUse: (password: string) => void; onOpenSettings?: () => void }) {
  const saved = useSettings().data?.generator;
  const limits = useQuery({ queryKey: keys.generatorLimits, queryFn: api.generatorLimits }).data;
  const [options, setOptions] = useState<GeneratorOptions>(() => saved ?? FALLBACK);
  const [result, setResult] = useState<GeneratedPassword | { error: string } | null>(null);
  const asked = useRef(0);
  const saveTimer = useRef<ReturnType<typeof setTimeout>>(undefined);

  const generate = async (with_: GeneratorOptions) => {
    const ask = ++asked.current;
    try {
      const generated = await api.generatePassword(with_);
      if (ask === asked.current) setResult(generated);
    } catch (error) {
      if (ask === asked.current) setResult({ error: errorMessage(error) });
    }
  };

  useEffect(() => {
    void generate(options);
    return () => clearTimeout(saveTimer.current);
    // Only on opening: changes generate as they are made.
  }, []);

  /** Changes the settings: generates with them, and remembers them in the app config. */
  const change = (next: GeneratorOptions) => {
    setOptions(next);
    void generate(next);
    clearTimeout(saveTimer.current);
    saveTimer.current = setTimeout(() => void api.setGeneratorSettings(next).catch(() => {}), 250);
  };

  const password = result && "password" in result ? result.password : null;
  const strength = result && "strength" in result ? result.strength : null;
  const error = result && "error" in result ? result.error : null;
  const passphrase = options.mode === "passphrase";
  const count = passphrase ? options.passphrase.words : options.characters.length;
  const setCount = (value: number) =>
    change(
      passphrase
        ? { ...options, passphrase: { ...options.passphrase, words: value } }
        : { ...options, characters: { ...options.characters, length: value } },
    );
  const range = sliderRange(options, limits);
  const shown = Math.min(Math.max(count, range.min), range.max);
  const fill = `${((shown - range.min) / Math.max(1, range.max - range.min)) * 100}%`;

  return (
    <div
      onKeyDown={(event) => {
        const target = event.target as HTMLElement;
        if (event.key === "Enter" && !(target instanceof HTMLButtonElement)) {
          event.preventDefault();
          if (password !== null) onUse(password);
        } else if ((event.key === "r" || event.key === "R") && !event.ctrlKey && !event.metaKey && !isTyping(target)) {
          event.preventDefault();
          void generate(options);
        }
      }}
    >
      <div className="gp-head">
        <Segmented
          label="Kind of password"
          value={options.mode}
          onValueChange={(mode) => change({ ...options, mode })}
          options={[
            { value: "characters", label: <><CaseSensitive className="ico-sm" />Characters</> },
            { value: "passphrase", label: <><WholeWord className="ico-sm" />Passphrase</> },
          ]}
        />
        <span className="spacer" />
        {onOpenSettings && (
          <Button variant="ghost" size="sm" title="The generator's defaults, in General Settings" onClick={onOpenSettings}>
            <SlidersHorizontal /> Settings…
          </Button>
        )}
      </div>
      <div className="gp-prev">
        <div className={cn("pv", error && "err")} aria-live="polite">
          {error ? (
            <>
              <CircleAlert />
              <span>{error}</span>
            </>
          ) : password !== null ? (
            segments(password, options).map((segment, index) => (
              <span key={index} className={segment.kind === "digit" ? "d" : segment.kind === "symbol" ? "s" : segment.kind === "separator" ? "sep" : undefined}>
                {segment.text}
              </span>
            ))
          ) : null}
        </div>
        <div className="gp-prev-foot">
          <StrengthMeter
            strength={strength}
            extra={error ? undefined : passphrase ? `${options.passphrase.words} words` : `${options.characters.length} characters`}
            className="gp-strength"
          />
          <span className="spacer" />
          <button type="button" className="btn ghost sm icon" aria-label="Regenerate" title="Regenerate (R)" disabled={!!error} onClick={() => void generate(options)}>
            <RefreshCw />
          </button>
        </div>
      </div>
      <div className="gp-body">
        <div className="vstack gap-2">
          <span className="gp-label">{passphrase ? "Words" : "Length"}</span>
          <div className="gp-row">
            <input
              type="range"
              className="gp-slider"
              aria-label={passphrase ? "Words" : "Length"}
              min={range.min}
              max={range.max}
              value={shown}
              style={{ "--fill": fill } as React.CSSProperties}
              onChange={(event) => setCount(Number(event.target.value))}
            />
            <NumberField
              label={passphrase ? "Words" : "Length"}
              value={count}
              max={passphrase ? (limits?.maxWords ?? 40) : (limits?.maxLength ?? 256)}
              onChange={setCount}
            />
          </div>
        </div>
        {passphrase ? (
          <div className="gp-row">
            <span className="gp-label flex-1">Separator</span>
            <label className="input mono gp-sep">
              <input
                aria-label="Separator"
                value={options.passphrase.separator}
                maxLength={3}
                spellCheck={false}
                onChange={(event) => change({ ...options, passphrase: { ...options.passphrase, separator: event.target.value } })}
              />
            </label>
            <div className="gp-seps">
              {SEPARATORS.map((separator) => (
                <button
                  key={separator}
                  type="button"
                  aria-pressed={options.passphrase.separator === separator}
                  aria-label={separator === " " ? "Space" : separator}
                  className={cn("gp-chip", options.passphrase.separator === separator && "is-on")}
                  onClick={() => change({ ...options, passphrase: { ...options.passphrase, separator } })}
                >
                  {separator === " " ? "␣" : separator}
                </button>
              ))}
            </div>
          </div>
        ) : (
          <>
            <div className="gp-chips">
              {CLASSES.map(([key, label]) => (
                <button
                  key={key}
                  type="button"
                  aria-pressed={options.characters[key]}
                  className={cn("gp-chip", options.characters[key] && "is-on")}
                  onClick={() => change(toggleClass(options, key))}
                >
                  {label}
                </button>
              ))}
            </div>
            <div className="gp-row">
              <Switch
                label="Exclude look-alikes"
                checked={options.characters.excludeLookalikes}
                onCheckedChange={(excludeLookalikes) => change({ ...options, characters: { ...options.characters, excludeLookalikes } })}
              />
              <span className="text-sm">Exclude look-alikes</span>
              <span className="gp-lookalikes">{limits?.lookalikes ?? "0O1Il|8B6G"}</span>
            </div>
          </>
        )}
      </div>
      <div className="gp-foot">
        <span className="hint hstack gap-1.5">
          <Kbd combo="Escape" /> close
        </span>
        <span className="spacer" />
        <Button variant="ghost" size="sm" disabled={!!error} onClick={() => void generate(options)}>
          Regenerate <kbd>R</kbd>
        </Button>
        <Button variant="primary" size="sm" disabled={password === null} onClick={() => password !== null && onUse(password)}>
          Use password <Kbd combo="Enter" />
        </Button>
      </div>
    </div>
  );
}

/** A small number field that lets the user type freely and says out-of-range values as the
 * generator's error rather than clamping them away. */
function NumberField({ label, value, max, onChange }: { label: string; value: number; max: number; onChange: (value: number) => void }) {
  const [text, setText] = useState(String(value));
  useEffect(() => setText(String(value)), [value]);
  return (
    <label className="input gp-num">
      <input
        aria-label={label}
        inputMode="numeric"
        value={text}
        onChange={(event) => {
          const typed = event.target.value.replace(/[^0-9]/g, "").slice(0, String(max).length + 1);
          setText(typed);
          if (typed !== "") onChange(Number(typed));
        }}
        onBlur={() => setText(String(value))}
      />
    </label>
  );
}
