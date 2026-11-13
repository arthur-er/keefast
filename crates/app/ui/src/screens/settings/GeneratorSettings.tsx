// General Settings' password generator: the settings keefast and the Companion extension
// generate passwords with, and a password made with them to show what they make.

import { type CSSProperties, useEffect, useState } from "react";
import { Check as CheckIcon, CircleAlert, RefreshCw } from "lucide-react";
import { useQuery, useQueryClient } from "@tanstack/react-query";

import { api, errorMessage } from "@/api";
import type { AppSettings, GeneratedPassword, GeneratorOptions } from "@/bindings";
import { keys } from "@/api/queries";
import { IconButton, Segmented, StrengthMeter } from "@/kit";
import { cn } from "@/lib/utils";

/** The sliders' ranges; the number fields go up to the generator's limits. */
const LENGTH_SLIDER = [4, 64] as const;
const WORDS_SLIDER = [3, 12] as const;

type Classes = "lowercase" | "uppercase" | "digits" | "symbols";
const CLASSES: readonly [Classes, string][] = [
  ["lowercase", "a–z"],
  ["uppercase", "A–Z"],
  ["digits", "0–9"],
  ["symbols", "!@#$"],
];

export function GeneratorSettings({ options }: { options: GeneratorOptions }) {
  const client = useQueryClient();
  const limits = useQuery({ queryKey: keys.generatorLimits, queryFn: api.generatorLimits }).data;
  const [sample, setSample] = useState<GeneratedPassword | { error: string } | null>(null);

  const regenerate = (settings: GeneratorOptions) =>
    api.generatePassword(settings).then(
      (generated) => setSample(generated),
      (error) => setSample({ error: errorMessage(error) }),
    );

  // A first password with the saved settings.
  useEffect(() => void regenerate(options), []);

  const change = (next: GeneratorOptions) => {
    client.setQueryData<AppSettings>(keys.settings, (settings) => settings && { ...settings, generator: next });
    void regenerate(next);
    void api.setGeneratorSettings(next).catch((error) => setSample({ error: errorMessage(error) }));
  };
  const characters = options.characters;
  const setCharacters = (patch: Partial<GeneratorOptions["characters"]>) => change({ ...options, characters: { ...characters, ...patch } });
  const setPassphrase = (patch: Partial<GeneratorOptions["passphrase"]>) => change({ ...options, passphrase: { ...options.passphrase, ...patch } });

  return (
    <>
      <div className="gen-preview">
        <div className="pw" aria-live="polite">
          {sample && "password" in sample ? (
            <Colored password={sample.password} plain={options.mode === "passphrase"} symbols={limits?.symbols} />
          ) : sample ? (
            <span className="error-msg font-[family-name:var(--font-ui)] text-sm">
              <CircleAlert className="ico-sm" />
              {sample.error}
            </span>
          ) : null}
        </div>
        <IconButton size="sm" label="Generate another" onClick={() => void regenerate(options)}>
          <RefreshCw />
        </IconButton>
      </div>
      {sample && "strength" in sample && (
        <div className="gen-meter">
          <StrengthMeter strength={sample.strength} />
        </div>
      )}
      <div className="gen-grid">
        <span className="k">Type</span>
        <div>
          <Segmented
            label="Type"
            value={options.mode}
            onValueChange={(mode) => change({ ...options, mode })}
            options={[
              { value: "characters", label: "Characters" },
              { value: "passphrase", label: "Passphrase" },
            ]}
          />
        </div>
        {options.mode === "characters" ? (
          <>
            <span className="k">Length</span>
            <NumberSlider
              label="Length"
              value={characters.length}
              slider={LENGTH_SLIDER}
              max={limits?.maxLength ?? 256}
              onChange={(length) => setCharacters({ length })}
            />
            <span className="k">Characters</span>
            <div className="chips" role="group" aria-label="Characters">
              {CLASSES.map(([key, label]) => (
                <button
                  key={key}
                  type="button"
                  role="checkbox"
                  aria-checked={characters[key]}
                  className={cn("chip", characters[key] && "is-on")}
                  onClick={() => setCharacters({ [key]: !characters[key] })}
                >
                  {characters[key] && <CheckIcon />}
                  {label}
                </button>
              ))}
            </div>
            <span />
            <label className="check-line">
              <button
                type="button"
                role="checkbox"
                aria-checked={characters.excludeLookalikes}
                className={cn("check", characters.excludeLookalikes && "is-on")}
                onClick={() => setCharacters({ excludeLookalikes: !characters.excludeLookalikes })}
              />
              Exclude look-alikes {limits && <span className="mono">({limits.lookalikes})</span>}
            </label>
          </>
        ) : (
          <>
            <span className="k">Words</span>
            <NumberSlider
              label="Words"
              value={options.passphrase.words}
              slider={WORDS_SLIDER}
              max={limits?.maxWords ?? 40}
              onChange={(words) => setPassphrase({ words })}
            />
            <span className="k">Separator</span>
            <div className="hstack">
              <div className="input mono w-16">
                <input
                  value={options.passphrase.separator}
                  maxLength={3}
                  aria-label="Separator"
                  spellCheck={false}
                  className="text-center"
                  onChange={(event) => setPassphrase({ separator: event.target.value })}
                />
              </div>
              <span className="hint">
                Between words, e.g. <span className="mono">-</span> <span className="mono">.</span> or a space
              </span>
            </div>
          </>
        )}
      </div>
    </>
  );
}

/** A slider over the usual range, and a field for any number up to `max`. */
function NumberSlider({
  label,
  value,
  slider: [min, top],
  max,
  onChange,
}: {
  label: string;
  value: number;
  slider: readonly [number, number];
  max: number;
  onChange: (value: number) => void;
}) {
  const [text, setText] = useState(String(value));
  useEffect(() => setText(String(value)), [value]);
  const shown = Math.min(Math.max(value, min), top);
  const valid = (n: number) => Number.isInteger(n) && n >= 1 && n <= max;
  return (
    <div className="slider">
      <input
        type="range"
        min={min}
        max={top}
        value={shown}
        aria-label={label}
        style={{ "--fill": `${((shown - min) / (top - min)) * 100}%` } as CSSProperties}
        onChange={(event) => onChange(Number(event.target.value))}
      />
      <div className={cn("input num mono", !valid(Number(text)) && "invalid")}>
        <input
          value={text}
          inputMode="numeric"
          aria-label={label}
          onChange={(event) => {
            setText(event.target.value);
            const n = Number(event.target.value);
            if (event.target.value.trim() && valid(n)) onChange(n);
          }}
          onBlur={() => setText(String(value))}
        />
      </div>
    </div>
  );
}

/** A generated password with its digits and symbols coloured, as the prototype shows it. */
function Colored({ password, plain, symbols }: { password: string; plain: boolean; symbols?: string }) {
  if (plain || !symbols) return <>{password}</>;
  return (
    <>
      {[...password].map((char, index) =>
        /[0-9]/.test(char) ? (
          <span key={index} className="d">
            {char}
          </span>
        ) : symbols.includes(char) ? (
          <span key={index} className="s">
            {char}
          </span>
        ) : (
          char
        ),
      )}
    </>
  );
}
