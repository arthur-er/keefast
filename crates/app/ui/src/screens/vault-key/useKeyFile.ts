// The Key file of a master key form: none, one chosen from disk, or a new one keefast writes.

import { useState } from "react";

import { api, errorMessage } from "@/api";

export interface KeyFileState {
  path: string | null;
  /** Whether keefast wrote it just now (Create new…), so the user should keep a copy. */
  created: boolean;
  /** Why choosing or writing it failed. */
  error: string | null;
  choose(): Promise<void>;
  /** Asks where to save a new Key file, by `near` (a Vault file), writes it and uses it. */
  create(near: string | null): Promise<void>;
  clear(): void;
}

export function useKeyFile(): KeyFileState {
  const [path, setPath] = useState<string | null>(null);
  const [created, setCreated] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const use = (picked: string | null, isNew: boolean) => {
    if (!picked) return;
    setPath(picked);
    setCreated(isNew);
    setError(null);
  };
  return {
    path,
    created,
    error,
    choose: async () => {
      try {
        use(await api.chooseKeyFile(), false);
      } catch (failure) {
        setError(errorMessage(failure));
      }
    },
    create: async (near) => {
      try {
        use(await api.createKeyFile(near?.trim() ? near.trim() : null), true);
      } catch (failure) {
        setError(errorMessage(failure));
      }
    },
    clear: () => {
      setPath(null);
      setCreated(false);
      setError(null);
    },
  };
}
