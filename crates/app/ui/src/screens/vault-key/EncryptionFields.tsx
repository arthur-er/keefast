// The cipher, key derivation and its numbers, stacked (New Vault's encryption box).

import { cn } from "@/lib/utils";

import { CIPHERS, type EncryptionField, type EncryptionForm, KDFS } from "./encryption";

export function EncryptionFields({
  form,
  onChange,
  invalid,
  disabled,
}: {
  form: EncryptionForm;
  onChange: (form: EncryptionForm) => void;
  /** The field to mark, or `all` the KDF's numbers. */
  invalid: EncryptionField | "all" | null;
  disabled?: boolean;
}) {
  const number = (field: EncryptionField, label: string, unit?: string) => (
    <div className="field nv-num">
      <label htmlFor={`enc-${field}`}>{label}</label>
      <div className={cn("input", (invalid === field || invalid === "all") && "invalid")}>
        <input
          id={`enc-${field}`}
          inputMode="numeric"
          value={form[field]}
          disabled={disabled}
          onChange={(event) => onChange({ ...form, [field]: event.target.value })}
        />
        {unit && <span className="unit">{unit}</span>}
      </div>
    </div>
  );
  return (
    <>
      <div className="field">
        <label>Cipher</label>
        <Choice label="Cipher" options={CIPHERS} value={form.cipher} disabled={disabled} onChange={(cipher) => onChange({ ...form, cipher })} />
      </div>
      <div className="field">
        <label>Key derivation</label>
        <Choice label="Key derivation" options={KDFS} value={form.kdf} disabled={disabled} onChange={(kdf) => onChange({ ...form, kdf })} />
      </div>
      <div className="nv-grid3">
        {form.kdf === "aesKdf" ? (
          number("rounds", "Rounds")
        ) : (
          <>
            {number("memoryMib", "Memory (MiB)", "MiB")}
            {number("iterations", "Iterations")}
            {number("parallelism", "Parallelism")}
          </>
        )}
      </div>
    </>
  );
}

/** `.seg` stretched to its box, one button per option. */
export function Choice<Value extends string>({
  label,
  options,
  value,
  onChange,
  disabled,
  full = true,
}: {
  label: string;
  options: readonly { value: Value; label: string }[];
  value: Value;
  onChange: (value: Value) => void;
  disabled?: boolean;
  full?: boolean;
}) {
  return (
    <div className={cn("seg", full && "full")} role="radiogroup" aria-label={label}>
      {options.map((option) => (
        <button
          key={option.value}
          type="button"
          role="radio"
          aria-checked={option.value === value}
          disabled={disabled}
          className={cn(option.value === value && "is-on")}
          onClick={() => onChange(option.value)}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}
