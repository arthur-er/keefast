// Vault settings C's "Security" section: what it takes to unlock the Vault (its cipher and key
// derivation, from `get_vault_settings`), and the way to change its master key and encryption.
//
// The change-key card itself (the prototype's `vault-settings.html?variant=C&state=key`) is
// Tauri UI 5/7's, built as its own component. It drops in at `KeyEditor` below: set it to that
// component, and "Change…" opens it in this section's card in place of the summary (Vault
// settings opened at the "key" section open it at once).

import type { ComponentType } from "react";

import type { EncryptionSettings, VaultId, VaultSettings } from "@/bindings";
import { Button } from "@/kit";

import { SettingRow } from "./SettingsLayout";

/** What the change-key card gets: the Vault, its settings now, and how to close it (after
 * saving, or cancelled). */
export interface KeyEditorProps {
  vault: VaultId;
  settings: VaultSettings;
  onClose: () => void;
}

/** Tauri UI 5/7's change-key card; `null` until it lands, when "Change…" says it is coming. */
export const KeyEditor: ComponentType<KeyEditorProps> | null = null;

const CIPHERS: Record<EncryptionSettings["cipher"], string> = {
  aes256: "AES-256",
  chaCha20: "ChaCha20",
  twofish: "Twofish",
};

/** The encryption in a line: "AES-256 · Argon2id · 64 MiB · 10 iterations · 2 lanes". */
export function describeEncryption({ cipher, kdf }: EncryptionSettings): string {
  const parts = [CIPHERS[cipher]];
  if (kdf.kind === "aesKdf") {
    parts.push("AES-KDF", `${kdf.rounds.toLocaleString("en")} rounds`);
  } else {
    const memory = kdf.memoryKib % 1024 === 0 ? `${kdf.memoryKib / 1024} MiB` : `${kdf.memoryKib} KiB`;
    parts.push(
      kdf.kind === "argon2id" ? "Argon2id" : "Argon2d",
      memory,
      `${kdf.iterations} ${kdf.iterations === 1 ? "iteration" : "iterations"}`,
      `${kdf.parallelism} ${kdf.parallelism === 1 ? "lane" : "lanes"}`,
    );
  }
  return parts.join(" · ");
}

/** The search's words for the section. */
export const KEY_SEARCH = "Master key and encryption password key file cipher kdf argon2 aes chacha20 twofish change master unlock";

/** The summary row: the encryption, and "Change…". */
export function VaultKeySection({ settings, onChange }: { settings: VaultSettings; onChange: () => void }) {
  return (
    <SettingRow label="Master key and encryption" desc={describeEncryption(settings.encryption)}>
      <Button size="sm" onClick={onChange}>
        Change…
      </Button>
    </SettingRow>
  );
}
