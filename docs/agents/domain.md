# Domain Docs

Before exploring an area, read:

- **`GLOSSARY.md`** at the repo root: keefast's terms.
- **`docs/adr/`**: the ADRs that touch the area.

When your output names a domain concept (an issue title, a proposal, a test name), use the
glossary's term, not a synonym it lists under _Avoid_. A concept the glossary lacks means you're
inventing language the project doesn't use, or there's a gap to fill with `/domain-modeling`.

If your output contradicts an ADR, say so explicitly rather than silently overriding it:

> _Contradicts ADR 0012 (Agents), but worth reopening because…_
