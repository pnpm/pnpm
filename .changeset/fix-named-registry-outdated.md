---
"pacquet": patch
---

Fixed `pnpm outdated` and `pnpm update --interactive --latest` omitting named-registry dependencies such as `work:2.1.0`. Updating these dependencies with `--latest` now preserves their registry prefix [pnpm/pnpm#15226](https://github.com/pnpm/pnpm/issues/15226).
