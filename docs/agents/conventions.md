# Conventions

## Commits, branches and pull requests

- Commit subjects are imperative, plain-English sentences about the effect, with no
  `feat:`-style prefix and no trailing period. They use the glossary's terms and are often long
  and specific: "Add Start on boot: an autostart entry that starts keefast hidden", "Record how
  Agents reach secrets through MCP (ADR 0012)".
- Bodies, when there are any, say why, in prose or `- ` bullets, and reference issues
  ("Part of #166", "Closes #165").
- Branches: `feat/…`, `fix/…`, `refactor/…`, `chore/…`, `test/…`, `ci/…`, `docs/…`. Pull
  requests are merged with merge commits; main is merged into a branch, not rebased onto.
- Issues are on GitHub (`docs/agents/issue-tracker.md`, `docs/agents/triage-labels.md`).

## Words

`GLOSSARY.md` defines keefast's terms (`**Term**:` definition, `_Avoid_:` synonyms). Code, UI
strings, tests and commit messages use them, capitalised as nouns: Vault, Entry, Group, Master
key, Key file, Agent pairing, Headless host, Approver, Merge, Storage provider.

## Docs

- ADRs are `docs/adr/NNNN-sentence-title.md`, with the decision as the H1 sentence, in prose.
  A later change that extends one is appended as `## Update YYYY-MM-DD: …` rather than a new ADR.
- `docs/agents/` is for coding agents' configuration; product docs go elsewhere in `docs/`.

## Code

- Each file starts with a `//!` paragraph saying what it is for, with its ADRs. Comments say
  why, in full sentences.
- Dependencies are declared once, in the root `Cargo.toml`, with a comment when the reason
  isn't obvious.
- Errors are a `thiserror` enum per area; the app maps them to UI errors in
  `crates/app/src/error.rs`.
- `core` has no I/O and no async runtime. `workspace` is headless and synchronous, with its
  collaborators (`StorageProvider`, `Clock`, `Clipboard`, `ConfigStore`…) injected as
  `Arc<dyn Trait>`. `agent` sits on `workspace` and links no Tauri or WebKit.

## Tests

- Integration tests are `crates/<crate>/tests/<topic>.rs`, one topic per file, starting with a
  `//!` paragraph; shared helpers in `tests/support/`. Test names are behaviour sentences:
  `the_first_access_asks_the_user_about_the_undecided_entries`.
- Drive behaviour through the highest seam and assert what the user, extension or Agent sees.
  Fake only process and network boundaries: `FakeStorage`, `FakeClock::at_unix_seconds(..)`,
  `FakeClipboard`, `FakeConfigStore`, `FakeDropbox` (`crates/workspace/tests/support/`).
- Fixtures: `fixtures/*.kdbx` is the real-client corpus (master password `fixture-password`),
  key files in `fixtures/key-files/`; regenerate with
  `nix develop .#corpus -c scripts/make-fixtures.sh`.
- Tests run in parallel on loaded machines and on CI's Windows runner, so:
  - A server in a test binds port 0 (`127.0.0.1:0`, `Listen::new("127.0.0.1:0", ..)`) and
    reads back the port it got. Never a fixed port.
  - A token, timeout or lifetime a test waits on is 3 seconds or more, not 1.
  - Unix-only tests are `#[cfg(unix)]`.
- UI tests are Vitest with jsdom, next to the component (`*.test.tsx`). The mock backend
  (`crates/app/ui/src/api/mock/`) implements every command; `pnpm typecheck` fails until a new
  one is there.
