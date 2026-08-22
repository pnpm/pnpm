---
"@pnpm/exec.commands": patch
"pnpm": patch
"pacquet": patch
---

Support both alphabetical and `package.json` insertion order for regex-matched scripts: non-sequential runs sort alphabetically for deterministic parallelism, while `--sequential` preserves insertion order ([#13174](https://github.com/pnpm/pnpm/issues/13174)). Add `/regexp/` script selector to the Rust CLI for parity.
