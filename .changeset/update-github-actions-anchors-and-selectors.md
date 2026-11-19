---
"pacquet": patch
---

Fixed `pnpm update --include-github-actions` panicking or corrupting workflow YAML when steps or mappings use YAML anchors, aliases, or merge keys. `pnpm update` and `pnpm outdated` also no longer match every Cargo, Python, or npm dependency when combining a positive ecosystem selector with a negated package selector.
