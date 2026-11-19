# keefast

## Agent skills

### Issue tracker

Issues are tracked in GitHub Issues (`arthur-er/keefast`) via the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Triage labels

Default vocabulary: `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: one `GLOSSARY.md` and `docs/adr/` at the repo root. See `docs/agents/domain.md`.

## Commands

- Cargo only through the dev shell: `nix develop -c cargo …`. Test one crate at a time
  (`-p keefast-core`, `-p keefast-agent`…); `-p keefast` and `--workspace` build Tauri.
  Details: `docs/agents/build-test.md`.
- After changing a command, DTO or event in `crates/app/src`: `pnpm bindings` in
  `crates/app/ui`, and commit `src/bindings.ts`.
- Before pushing: `scripts/check.sh` runs what CI runs (`--crate <name>`, `--no-ui`… to narrow
  it; `--help`).
- Conventions for commits, docs, code and tests: `docs/agents/conventions.md`.

## Worktrees

- `scripts/setup-worktree.sh` readies a fresh worktree (t3.json runs it): the UI's
  dependencies, the pre-commit hook, and the worktree's mock UI port.
- `scripts/ui-mock.sh start|stop|status` serves the UI over the mock backend on that port.
  Never `pkill -f vite`: it kills your own shell and other worktrees' servers.
- Each `target/` grows past 20 GB: delete yours when you are done, or share an idle one through
  `CARGO_TARGET_DIR`.
