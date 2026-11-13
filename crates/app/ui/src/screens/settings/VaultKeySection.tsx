// Vault settings C's "Security" section: what it takes to unlock the Vault (what its master key
// is made of, and its cipher and key derivation, from `get_vault_settings`), and changing its master key and encryption. "Change…"
// opens the change-key card (`screens/vault-key/`, the prototype's
// `vault-settings.html?variant=C&state=key`) in this section's card, in place of the summary row;
// Vault settings opened at the "key" section open it at once.

import type { VaultId, VaultSettings } from "@/bindings";
import { Button } from "@/kit";
import { ChangeVaultKey } from "@/screens/vault-key/ChangeVaultKey";
import { encryptionSummary } from "@/screens/vault-key/encryption";
import { masterKeyName } from "@/screens/vault-key/masterKey";

import { SettingRow } from "./SettingsLayout";

/** The search's words for the section. */
export const KEY_SEARCH = "Master key and encryption password key file cipher kdf argon2 aes chacha20 twofish change master unlock";

/** The Security card's content: the summary row with "Change…", or the change-key card. */
export function VaultKeySection({
  vault,
  settings,
  open,
  onOpen,
  onClose,
}: {
  vault: VaultId;
  settings: VaultSettings;
  open: boolean;
  onOpen: () => void;
  onClose: () => void;
}) {
  if (open) {
    // The card's styles are scoped under `.vk` (screens/vault-key/vault-key.css).
    return (
      <div className="vk">
        <ChangeVaultKey vault={vault} onClose={onClose} />
      </div>
    );
  }
  return (
    <SettingRow label="Master key and encryption" desc={keySummary(settings)}>
      <Button size="sm" onClick={onOpen}>
        Change…
      </Button>
    </SettingRow>
  );
}

/** "Master password + Key file · AES-256 · Argon2id · 64 MiB": the master key and encryption. */
export function keySummary(settings: Pick<VaultSettings, "masterKey" | "encryption">): string {
  return [masterKeyName(settings.masterKey), ...encryptionSummary(settings.encryption).slice(0, 3)].join(" · ");
}
