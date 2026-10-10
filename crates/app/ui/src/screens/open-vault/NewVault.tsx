// Open a Vault B's New Vault state: where the Vault file goes and its name, the master key (a
// master password typed twice, a Key file, or both), and the encryption, collapsed to a summary
// with keefast's defaults. keefast never writes a new Vault over an existing file.

import { useEffect, useRef, useState } from "react";
import { Check, ChevronRight, CircleAlert, Eye, EyeOff, FileKey2, FileLock2, LoaderCircle, Sparkles, X } from "lucide-react";

import { api, ApiError, errorMessage } from "@/api";
import type { EncryptionSettings } from "@/bindings";
import { Badge, Button, Kbd } from "@/kit";
import { useShortcut } from "@/lib/shortcuts";
import { cn } from "@/lib/utils";
import { EncryptionFields } from "@/screens/vault-key/EncryptionFields";
import { checkEncryption, type EncryptionField, encryptionForm, type EncryptionForm, formSummary } from "@/screens/vault-key/encryption";
import { checkKey, fileName, matchState } from "@/screens/vault-key/masterKey";
import { useKeyFile } from "@/screens/vault-key/useKeyFile";

/** What's wrong, and where to show it: by a field, or under the form. */
type Problem = { message: string; detail?: string | null; at: "path" | "password" | "confirm" | "encryption" | "form"; field?: EncryptionField | null };

/** keefast's defaults, until the backend says them. */
const FALLBACK: EncryptionSettings = { cipher: "aes256", kdf: { kind: "argon2id", memoryKib: 65536, iterations: 5, parallelism: 4 } };

export function NewVault({ active = true, onDone, onCancel }: { active?: boolean; onDone: () => void; onCancel: () => void }) {
  const [path, setPath] = useState("");
  const [name, setName] = useState("");
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [shown, setShown] = useState(false);
  const keyFile = useKeyFile();
  const [defaults, setDefaults] = useState<EncryptionSettings>(FALLBACK);
  const [encryption, setEncryption] = useState<EncryptionForm>(() => encryptionForm(FALLBACK));
  const [encryptionOpen, setEncryptionOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<Problem | null>(null);
  const edited = useRef(false);

  useEffect(() => {
    void api
      .defaultEncryption()
      .then((settings) => {
        setDefaults(settings);
        if (!edited.current) setEncryption(encryptionForm(settings));
      })
      .catch(() => {});
  }, []);

  const editEncryption = (form: EncryptionForm) => {
    edited.current = true;
    setEncryption(form);
    if (problem?.at === "encryption") setProblem(null);
  };

  const create = async () => {
    if (busy) return;
    if (!path.trim()) return setProblem({ message: "Choose where to save the Vault file.", at: "path" });
    const key = checkKey(password, confirm, keyFile.path);
    if (!key.ok) return setProblem({ message: key.message, at: key.field });
    const checked = checkEncryption(encryption, defaults);
    if (!checked.ok) {
      setEncryptionOpen(true);
      return setProblem({ message: checked.message, at: "encryption", field: checked.field });
    }
    setBusy(true);
    setProblem(null);
    try {
      await api.createVault({ path: path.trim(), name: name.trim(), key: key.key, encryption: checked.settings });
      // The master password is only needed until the Vault is written (ADR 0004).
      setPassword("");
      setConfirm("");
      onDone();
    } catch (failure) {
      const error = failure instanceof ApiError ? failure : null;
      const atPath = error?.kind === "conflict" || error?.kind === "alreadyOpen";
      setProblem({ message: errorMessage(failure), detail: error?.detail, at: atPath ? "path" : "form" });
    } finally {
      setBusy(false);
    }
  };

  useShortcut("Ctrl Enter", () => void create(), { enabled: active });

  const match = matchState(password, confirm);
  const summary = formSummary(encryption, defaults);
  const fieldError = (at: Problem["at"]) =>
    problem?.at === at && (
      <div className="error-msg" role="alert">
        <CircleAlert className="ico-sm" /> {problem.message}
      </div>
    );

  return (
    <div className="b-stage" style={{ placeItems: "start center" }}>
      <form
        className="b-nv"
        noValidate
        onSubmit={(event) => {
          event.preventDefault();
          void create();
        }}
      >
        <h2>Create a Vault</h2>
        <p className="lede">A new Vault file, encrypted with a master key only you hold.</p>

        <section className="b-nv-sec">
          <div>
            <div className="nv-section-title">Location</div>
            <div className="nv-section-desc">Where the Vault file is saved, and what keefast calls it.</div>
          </div>
          <div className="b-nv-ctl">
            <div className="field">
              <label htmlFor="nv-path">Vault file</label>
              <div className="hstack">
                <div className={cn("input mono flex-1", problem?.at === "path" && "invalid")}>
                  <FileLock2 className="ico-sm" />
                  <input
                    id="nv-path"
                    value={path}
                    placeholder="e.g. ~/Vaults/Personal.kdbx"
                    spellCheck={false}
                    autoComplete="off"
                    autoFocus
                    disabled={busy}
                    onChange={(event) => {
                      setPath(event.target.value);
                      if (problem?.at === "path") setProblem(null);
                    }}
                  />
                </div>
                <Button
                  disabled={busy}
                  onClick={async () => {
                    const picked = await api.chooseNewVaultFile(path.trim() || null).catch(() => null);
                    if (!picked) return;
                    setPath(picked);
                    if (problem?.at === "path") setProblem(null);
                  }}
                >
                  Browse…
                </Button>
              </div>
              {fieldError("path")}
            </div>
            <div className="field">
              <label htmlFor="nv-name">Name</label>
              <input
                id="nv-name"
                className="input"
                value={name}
                placeholder="The Vault file's name if empty"
                disabled={busy}
                onChange={(event) => setName(event.target.value)}
              />
            </div>
          </div>
        </section>

        <section className="b-nv-sec">
          <div>
            <div className="nv-section-title">Master key</div>
            <div className="nv-section-desc">A master password, a Key file, or both.</div>
          </div>
          <div className="b-nv-ctl">
            <div className="field">
              <label htmlFor="nv-password">Master password</label>
              <div className="pw">
                <div className={cn("input pr-1", problem?.at === "password" && "invalid")}>
                  <input
                    id="nv-password"
                    type={shown ? "text" : "password"}
                    className={cn(shown && "revealed")}
                    value={password}
                    placeholder="Type your master password"
                    spellCheck={false}
                    autoComplete="new-password"
                    disabled={busy}
                    onChange={(event) => {
                      setPassword(event.target.value);
                      if (problem?.at === "password" || problem?.at === "confirm") setProblem(null);
                    }}
                  />
                  <button
                    type="button"
                    className="btn ghost icon sm eye"
                    aria-label={shown ? "Hide the master password" : "Show the master password"}
                    title={shown ? "Hide password" : "Show password"}
                    onClick={() => setShown(!shown)}
                  >
                    {shown ? <EyeOff /> : <Eye />}
                  </button>
                </div>
              </div>
              {fieldError("password")}
            </div>
            <div className="field">
              <label htmlFor="nv-confirm">Repeat the master password</label>
              <div className="pw">
                <div className={cn("input", (problem?.at === "confirm" || match === "mismatch") && "invalid")}>
                  <input
                    id="nv-confirm"
                    type={shown ? "text" : "password"}
                    className={cn(shown && "revealed")}
                    value={confirm}
                    placeholder="Type it again"
                    spellCheck={false}
                    autoComplete="new-password"
                    disabled={busy}
                    onChange={(event) => {
                      setConfirm(event.target.value);
                      if (problem?.at === "confirm") setProblem(null);
                    }}
                  />
                </div>
              </div>
              {match === "match" ? (
                <span className="match ok">
                  <Check /> The two master passwords match.
                </span>
              ) : match === "mismatch" && problem?.at !== "confirm" ? (
                <div className="error-msg">
                  <CircleAlert className="ico-sm" /> The two master passwords are different.
                </div>
              ) : (
                fieldError("confirm")
              )}
            </div>
            <div className="field">
              <label>Key file</label>
              <div className="hstack">
                {keyFile.path ? (
                  <>
                    <span className="keyfile-chip" title={keyFile.path}>
                      <FileKey2 />
                      <span className="truncate">{fileName(keyFile.path)}</span>
                      <button type="button" className="x" aria-label="Remove the Key file" title="Remove the Key file" onClick={keyFile.clear}>
                        <X className="ico-sm" />
                      </button>
                    </span>
                    {keyFile.created && <Badge tone="accent">New</Badge>}
                  </>
                ) : (
                  <span className="subtle text-sm">None (optional)</span>
                )}
                <span className="spacer" />
                <Button size="sm" disabled={busy} onClick={() => void keyFile.choose()}>
                  Choose…
                </Button>
                <Button size="sm" disabled={busy} onClick={() => void keyFile.create(path)}>
                  <Sparkles /> Create new…
                </Button>
              </div>
              {keyFile.error && (
                <div className="error-msg" role="alert">
                  <CircleAlert className="ico-sm" /> {keyFile.error}
                </div>
              )}
              <div className="hint">
                Use a master password, a Key file, or both. Keep a copy of the Key file somewhere safe: without it the Vault can't be opened.
              </div>
            </div>
          </div>
        </section>

        <section className="b-nv-sec">
          <div>
            <div className="nv-section-title">Encryption</div>
            <div className="nv-section-desc">The defaults are read by every KeePass client.</div>
          </div>
          <div className="b-nv-ctl">
            <div className="enc-box">
              <button
                type="button"
                className={cn("enc-head", encryptionOpen && "open")}
                aria-expanded={encryptionOpen}
                onClick={() => setEncryptionOpen(!encryptionOpen)}
              >
                <ChevronRight className="chev ico-sm" />
                <span className="enc-sum">
                  {summary.map((part, index) => (
                    <span key={index} className="hstack gap-1.5">
                      {index > 0 && <i className="sep" />}
                      {part}
                    </span>
                  ))}
                </span>
                <span className="subtle ml-auto text-sm">{encryptionOpen ? "Hide" : "Customize"}</span>
              </button>
              {encryptionOpen && (
                <div className="enc-body">
                  <EncryptionFields
                    form={encryption}
                    onChange={editEncryption}
                    invalid={problem?.at === "encryption" ? (problem.field ?? "all") : null}
                    disabled={busy}
                  />
                  {fieldError("encryption")}
                  <div className="hint">Higher values make guessing the master password slower, and unlocking and saving too.</div>
                </div>
              )}
            </div>
          </div>
        </section>

        <div className="b-nv-foot">
          <div className="left min-w-0 flex-1">
            {busy ? (
              <div className="vstack gap-2">
                <div className="progress">
                  <i />
                </div>
                <div className="hint">Creating the Vault: deriving the key…</div>
              </div>
            ) : (
              problem?.at === "form" && (
                <div className="notice danger" role="alert">
                  <CircleAlert />
                  <span>
                    {problem.message}
                    {problem.detail && <span className="detail">{problem.detail}</span>}
                  </span>
                </div>
              )
            )}
          </div>
          <Button variant="ghost" onClick={onCancel} disabled={busy} shortcut="Escape">
            Cancel
          </Button>
          <Button type="submit" variant="primary" disabled={busy}>
            {busy ? (
              <>
                <LoaderCircle className="spin" /> Creating…
              </>
            ) : (
              <>
                Create <Kbd combo="Ctrl Enter" />
              </>
            )}
          </Button>
        </div>
      </form>
    </div>
  );
}
